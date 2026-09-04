//! Sequence diagrams: `sequenceDiagram`.
//!
//! A column for each participant with a lifeline down it, and a row for each message. Everything
//! else — activation bars, notes, `loop` and `alt` frames, `box` bands — is placed against those two.
//!
//! The layout is two passes and no more. The first walks the events working out how tall each row
//! is and how far apart the columns have to be; the second draws. That is enough because nothing in
//! a sequence diagram moves anything else: a message goes between two known columns at a known
//! height, and a frame is drawn round rows that have already been measured.
//!
//! **A participant that is never declared still gets a column**, in the order it is first mentioned.
//! Mermaid does this and it is what makes a five-line sequence diagram work without a preamble.

use std::collections::HashMap;

use super::parts::{self, Ending};
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Line, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How far apart two columns are, at the least.
const COLUMN_GAP: f32 = 40.0;
/// How tall a participant's box is.
const HEAD_HEIGHT: f32 = 38.0;
/// How tall an ordinary message row is.
const ROW: f32 = 40.0;
/// How wide an activation bar is.
const BAR: f32 = 10.0;
/// How far a block frame is inset from the columns it covers.
const FRAME_INSET: f32 = 14.0;
/// The room a block's own header takes above its first row.
const FRAME_HEADER: f32 = 26.0;

/// How a message's line is drawn and what is on its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Arrow {
    dotted: bool,
    head: Ending,
    /// `<<->>` and `<<-->>` point both ways.
    both: bool,
}

/// One thing that happens, in the order it was written.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Message { from: usize, to: usize, arrow: Arrow, text: String },
    Note { over: Vec<usize>, side: Side, text: String },
    /// `loop`, `alt`, `opt`, `par`, `critical`, `break`, `rect`.
    Open { kind: String, label: String },
    /// `else`, `and`, `option`.
    Divide { label: String },
    Close,
    Activate(usize),
    Deactivate(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Over,
}

/// One participant.
#[derive(Debug, Clone, PartialEq)]
struct Participant {
    label: String,
    /// True for `actor`, which is drawn as a figure rather than as a box.
    is_actor: bool,
    /// The `box` it was declared inside, if any.
    band: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    participants: Vec<Participant>,
    by_id: HashMap<String, usize>,
    events: Vec<Event>,
    bands: Vec<String>,
    /// Set by `autonumber`, and counted up as the messages are drawn.
    numbered: bool,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    let mut band: Option<usize> = None;
    // `box` and the block words both end with `end`, so which one an `end` closes has to be
    // remembered rather than guessed.
    let mut open: Vec<bool> = Vec::new();
    for line in source.statements() {
        if line.starts_with_word("end") {
            match open.pop() {
                Some(true) => band = None,
                Some(false) => diagram.events.push(Event::Close),
                None => {}
            }
            continue;
        }
        if let Some(rest) = line.after_word("box") {
            diagram.bands.push(band_title(rest));
            band = Some(diagram.bands.len() - 1);
            open.push(true);
            continue;
        }
        if read_participant(&mut diagram, line, band) {
            continue;
        }
        if line.starts_with_word("autonumber") {
            diagram.numbered = true;
            continue;
        }
        if let Some(event) = read_block(line) {
            if matches!(event, Event::Open { .. }) {
                open.push(false);
            }
            diagram.events.push(event);
            continue;
        }
        if let Some(rest) = line.after_word("activate") {
            let who = participant(&mut diagram, rest, false, band);
            diagram.events.push(Event::Activate(who));
            continue;
        }
        if let Some(rest) = line.after_word("deactivate") {
            let who = participant(&mut diagram, rest, false, band);
            diagram.events.push(Event::Deactivate(who));
            continue;
        }
        if read_note(&mut diagram, line, band) {
            continue;
        }
        // `link Alice: Dashboard @ https://...` and `links Alice: {...}` are menus on an actor,
        // which a picture has no way to open. Read and ignored.
        if line.starts_with_word("link") || line.starts_with_word("links") {
            continue;
        }
        read_message(&mut diagram, line, band)?;
    }
    Ok(diagram)
}

/// `box Aqua The Team` — the colour word, if there is one, is not part of the name.
fn band_title(rest: &str) -> String {
    let rest = rest.trim();
    let first = rest.split_whitespace().next().unwrap_or_default();
    let is_colour = first.starts_with("rgb")
        || first.starts_with("hsl")
        || first.starts_with('#')
        || matches!(
            first.to_ascii_lowercase().as_str(),
            "aqua" | "transparent" | "red" | "green" | "blue" | "yellow" | "grey" | "gray"
        );
    if is_colour {
        source::label(&rest[first.len()..])
    } else {
        source::label(rest)
    }
}

/// Read `participant A as Alice`, `actor B`, `create participant C` and `destroy C`.
///
/// Returns true when the line was one of those. `create` and `destroy` change when a participant's
/// column appears and disappears in Mermaid; Unluminous draws every column for the whole height, so they
/// are read for the participant they name and their timing is not drawn.
fn read_participant(diagram: &mut Diagram, line: &Line, band: Option<usize>) -> bool {
    let mut rest = line.text.as_str();
    for word in ["create", "destroy"] {
        if let Some(after) = line.after_word(word) {
            rest = after;
        }
    }
    let (rest, is_actor) = if let Some(after) = strip_word(rest, "participant") {
        (after, false)
    } else if let Some(after) = strip_word(rest, "actor") {
        (after, true)
    } else {
        // A bare `destroy Bob` names a participant and nothing else.
        if rest != line.text {
            participant(diagram, rest, false, band);
            return true;
        }
        return false;
    };
    participant(diagram, rest, is_actor, band);
    true
}

/// What follows `word` when `text` begins with it as a whole word.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(word)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// Find or make the participant a piece of text names, reading `A as Alice` when it is there.
fn participant(
    diagram: &mut Diagram,
    text: &str,
    is_actor: bool,
    band: Option<usize>,
) -> usize {
    let text = text.trim();
    // An inline configuration object — `participant Alice, "alias": "A"` — is read for its name and
    // nothing else, because everything in it is about colour.
    let text = text.split(',').next().unwrap_or(text).trim();
    let (id, label) = match split_alias(text) {
        Some((id, label)) => (id.to_owned(), source::label(label)),
        None => (source::unquote(text), source::label(text)),
    };
    if let Some(&known) = diagram.by_id.get(&id) {
        if is_actor {
            diagram.participants[known].is_actor = true;
        }
        return known;
    }
    diagram.participants.push(Participant { label, is_actor, band });
    diagram.by_id.insert(id, diagram.participants.len() - 1);
    diagram.participants.len() - 1
}

/// Split `A as Alice` into its two halves, on the `as` that is a whole word.
fn split_alias(text: &str) -> Option<(&str, &str)> {
    let at = text.find(" as ")?;
    Some((text[..at].trim(), text[at + 4..].trim()))
}

/// Read the words that open or divide a block.
fn read_block(line: &Line) -> Option<Event> {
    for word in ["loop", "alt", "opt", "par", "critical", "break", "rect"] {
        if let Some(rest) = line.after_word(word) {
            return Some(Event::Open { kind: word.to_owned(), label: source::label(rest) });
        }
    }
    for word in ["else", "and", "option"] {
        if let Some(rest) = line.after_word(word) {
            return Some(Event::Divide { label: source::label(rest) });
        }
    }
    None
}

/// Read `Note left of A: text`, `Note right of A: text` and `Note over A,B: text`.
fn read_note(diagram: &mut Diagram, line: &Line, band: Option<usize>) -> bool {
    let Some(rest) = line.after_word("note").or_else(|| line.after_word("Note")) else {
        return false;
    };
    let (side, rest) = if let Some(rest) = strip_word(rest, "left of") {
        (Side::Left, rest)
    } else if let Some(rest) = strip_word(rest, "right of") {
        (Side::Right, rest)
    } else if let Some(rest) = strip_word(rest, "over") {
        (Side::Over, rest)
    } else {
        // `left of` and `right of` are two words, which `strip_word` will not take in one go.
        let lower = rest.to_ascii_lowercase();
        if let Some(after) = lower.strip_prefix("left of") {
            (Side::Left, &rest[rest.len() - after.len()..])
        } else if let Some(after) = lower.strip_prefix("right of") {
            (Side::Right, &rest[rest.len() - after.len()..])
        } else {
            return false;
        }
    };
    let (who, words) = match rest.split_once(':') {
        Some((who, words)) => (who, words),
        None => (rest, ""),
    };
    let over: Vec<usize> = source::split_outside_quotes(who, ',')
        .iter()
        .filter(|name| !name.trim().is_empty())
        .map(|name| participant(diagram, name, false, band))
        .collect();
    if over.is_empty() {
        return false;
    }
    diagram.events.push(Event::Note { over, side, text: source::label(words) });
    true
}

/// The arrow forms, longest first so `-->>` is never read as `-->` with a stray `>`.
const ARROWS: &[(&str, bool, Ending, bool)] = &[
    ("<<-->>", true, Ending::Arrow, true),
    ("<<->>", false, Ending::Arrow, true),
    ("-->>", true, Ending::Arrow, false),
    ("->>", false, Ending::Arrow, false),
    ("--)", true, Ending::None, false),
    ("-)", false, Ending::None, false),
    ("--x", true, Ending::Cross, false),
    ("-x", false, Ending::Cross, false),
    ("-->", true, Ending::None, false),
    ("->", false, Ending::None, false),
    ("--", true, Ending::None, false),
    ("-", false, Ending::None, false),
];

/// Read `Alice ->> Bob: Hello`.
fn read_message(diagram: &mut Diagram, line: &Line, band: Option<usize>) -> Result<(), Problem> {
    let (head, words) = match line.text.split_once(':') {
        Some((head, words)) => (head, source::label(words)),
        None => (line.text.as_str(), String::new()),
    };
    let Some((at, form)) = find_arrow(head) else {
        return Err(Problem::at(
            line,
            "this is not something a sequence diagram can do. A message looks like `Alice ->> Bob: Hello`.",
        ));
    };
    let (token, dotted, head_ending, both) = *form;
    let from = head[..at].trim();
    let mut to = head[at + token.len()..].trim();
    // `->>+` activates the receiver and `-->>-` deactivates the sender, which is the short way of
    // writing `activate` and `deactivate` round a message.
    let mut activates = false;
    let mut deactivates = false;
    while let Some(rest) = to.strip_prefix('+').or_else(|| to.strip_prefix('-')) {
        if to.starts_with('+') {
            activates = true;
        } else {
            deactivates = true;
        }
        to = rest.trim();
    }
    if from.is_empty() || to.is_empty() {
        return Err(Problem::at(line, "a message needs somebody at each end of it"));
    }
    let from = participant(diagram, from, false, band);
    let to = participant(diagram, to, false, band);
    if deactivates {
        diagram.events.push(Event::Deactivate(from));
    }
    diagram.events.push(Event::Message {
        from,
        to,
        arrow: Arrow { dotted, head: head_ending, both },
        text: words,
    });
    if activates {
        diagram.events.push(Event::Activate(to));
    }
    Ok(())
}

/// Where the arrow is in a message's head, and which form it is.
fn find_arrow(head: &str) -> Option<(usize, &'static (&'static str, bool, Ending, bool))> {
    let mut best: Option<(usize, &'static (&'static str, bool, Ending, bool))> = None;
    for form in ARROWS {
        let Some(at) = head.find(form.0) else {
            continue;
        };
        // The earliest arrow, and where two start at the same place the longest one, which is what
        // the ordering of `ARROWS` gives because the first match at a position wins.
        if best.is_none_or(|(known, _)| at < known) {
            best = Some((at, form));
        }
    }
    best
}

/// Where every column and every row went.
struct Frame {
    columns: Vec<f32>,
    heads: Vec<Rect>,
    /// The vertical middle of each event, in the order the events were read.
    rows: Vec<f32>,
    top: f32,
    bottom: f32,
    width: f32,
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    if diagram.participants.is_empty() {
        parts::finish(&mut scene);
        return scene;
    }
    let style = options.style(0.95, false);
    let heads: Vec<Label> = diagram
        .participants
        .iter()
        .map(|who| text::measure(&who.label, &style, options.metrics, 160.0))
        .collect();
    let messages: Vec<Label> = diagram
        .events
        .iter()
        .map(|event| measure_event(event, options))
        .collect();

    // An actor's figure is drawn above its name, so a diagram with one in it needs that much more
    // room at the top before the first row.
    let has_actor = diagram.participants.iter().any(|who| who.is_actor);
    let title = parts::title(&mut scene, source, options, 0.0)
        + if has_actor { HEAD_HEIGHT } else { 0.0 };
    let frame = measure_frame(diagram, &heads, &messages, title, options);

    draw_bands(&mut scene, diagram, &frame, options);
    draw_lifelines(&mut scene, diagram, &frame, options);
    draw_heads(&mut scene, diagram, &heads, &frame, options);
    draw_events(&mut scene, diagram, &messages, &frame, options);
    // Room at the bottom for the second row of heads and, when there is an actor, for its figure
    // under them.
    let below = HEAD_HEIGHT * if has_actor { 2.0 } else { 1.0 };
    scene.claim(Rect::new(0.0, 0.0, frame.width, frame.bottom + below + parts::MARGIN));
    parts::finish(&mut scene);
    scene
}

/// How much room one event's words take.
fn measure_event(event: &Event, options: &Options) -> Label {
    let style = options.style(0.85, false);
    let words = match event {
        Event::Message { text, .. } => text.as_str(),
        Event::Note { text, .. } => text.as_str(),
        Event::Open { label, .. } | Event::Divide { label } => label.as_str(),
        _ => "",
    };
    text::measure(words, &style, options.metrics, 200.0)
}

/// Work out where every column and every row goes.
fn measure_frame(
    diagram: &Diagram,
    heads: &[Label],
    messages: &[Label],
    title: f32,
    options: &Options,
) -> Frame {
    let widths: Vec<f32> = heads
        .iter()
        .map(|label| (label.width + parts::PADDING_X * 2.0).max(70.0))
        .collect();
    // A message's words have to fit between the two columns it joins, so the gap between any two
    // neighbouring columns is widened until the widest message crossing it fits.
    let mut gaps = vec![COLUMN_GAP; diagram.participants.len().saturating_sub(1)];
    for (index, event) in diagram.events.iter().enumerate() {
        let Event::Message { from, to, .. } = event else {
            continue;
        };
        let (left, right) = (*from.min(to), *from.max(to));
        if left == right {
            continue;
        }
        let wanted = messages[index].width + 24.0;
        let spanned: f32 = (left..right)
            .map(|at| gaps[at] + (widths[at] + widths[at + 1]) / 2.0)
            .sum();
        if wanted > spanned {
            let extra = (wanted - spanned) / (right - left) as f32;
            for gap in gaps.iter_mut().take(right).skip(left) {
                *gap += extra;
            }
        }
    }
    let mut columns = Vec::with_capacity(widths.len());
    let mut at = parts::MARGIN;
    for (index, width) in widths.iter().enumerate() {
        columns.push(at + width / 2.0);
        at += width + gaps.get(index).copied().unwrap_or(0.0);
    }
    let width = at + parts::MARGIN;
    let top = title + parts::MARGIN;
    let heads: Vec<Rect> = columns
        .iter()
        .zip(&widths)
        .map(|(centre, width)| Rect::new(centre - width / 2.0, top, *width, HEAD_HEIGHT))
        .collect();

    let mut rows = Vec::with_capacity(diagram.events.len());
    let mut y = top + HEAD_HEIGHT + 18.0;
    for (index, event) in diagram.events.iter().enumerate() {
        let height = row_height(event, &messages[index], options);
        y += height / 2.0;
        rows.push(y);
        y += height / 2.0;
    }
    Frame { columns, heads, rows, top, bottom: y + 12.0, width }
}

/// How tall the row for one event is.
fn row_height(event: &Event, label: &Label, options: &Options) -> f32 {
    match event {
        Event::Message { .. } => ROW.max(label.height + 26.0),
        Event::Note { .. } => (label.height + 26.0).max(ROW),
        Event::Open { .. } => FRAME_HEADER + label.height.max(options.base.size),
        Event::Divide { .. } => FRAME_HEADER,
        Event::Close => 14.0,
        Event::Activate(_) | Event::Deactivate(_) => 0.0,
    }
}

/// Draw the bands behind the participants a `box` grouped.
fn draw_bands(scene: &mut Scene, diagram: &Diagram, frame: &Frame, options: &Options) {
    for (index, title) in diagram.bands.iter().enumerate() {
        let members: Vec<usize> = (0..diagram.participants.len())
            .filter(|&who| diagram.participants[who].band == Some(index))
            .collect();
        let (Some(&first), Some(&last)) = (members.first(), members.last()) else {
            continue;
        };
        let rect = Rect::new(
            frame.heads[first].left() - 8.0,
            frame.top - 22.0,
            frame.heads[last].right() - frame.heads[first].left() + 16.0,
            frame.bottom - frame.top + 22.0 + HEAD_HEIGHT + 8.0,
        );
        scene.add(Item::Rect {
            rect,
            radius: parts::CORNER,
            fill: Some(options.theme.group_fill),
            stroke: Some(Stroke::new(options.theme.group_stroke, parts::LINE)),
        });
        let style = parts::text_style(options, 0.85, true, options.theme.dim);
        let width = text::width_of(title, &options.style(0.85, true), options.metrics);
        parts::one_line(
            scene,
            title,
            Point::new(rect.centre().x, rect.top() + 4.0),
            &style,
            Anchor::Middle,
            width,
        );
    }
}

/// Draw the dashed line down each participant, and the activation bars over it.
fn draw_lifelines(scene: &mut Scene, diagram: &Diagram, frame: &Frame, options: &Options) {
    let stroke = Stroke::new(options.theme.grid, parts::LINE);
    for column in &frame.columns {
        scene.add(Item::Line {
            points: vec![
                Point::new(*column, frame.top + HEAD_HEIGHT),
                Point::new(*column, frame.bottom + 6.0),
            ],
            stroke,
            dash: parts::DASH,
        });
    }
    for (who, top, bottom) in activations(diagram, frame) {
        scene.add(Item::Rect {
            rect: Rect::new(frame.columns[who] - BAR / 2.0, top, BAR, (bottom - top).max(ROW / 2.0)),
            radius: 2.0,
            fill: Some(Paint::solid(options.theme.accent)),
            stroke: Some(Stroke::new(options.theme.node_stroke, parts::LINE)),
        });
    }
}

/// Every activation bar, as who it belongs to and where it starts and stops.
///
/// An `activate` with no `deactivate` runs to the bottom, which is what Mermaid draws and is more
/// use than refusing the diagram over it.
fn activations(diagram: &Diagram, frame: &Frame) -> Vec<(usize, f32, f32)> {
    let mut open: HashMap<usize, Vec<f32>> = HashMap::new();
    let mut bars = Vec::new();
    for (index, event) in diagram.events.iter().enumerate() {
        let at = frame.rows.get(index).copied().unwrap_or(frame.bottom);
        match event {
            Event::Activate(who) => open.entry(*who).or_default().push(at - ROW / 2.0),
            Event::Deactivate(who) => {
                if let Some(from) = open.get_mut(who).and_then(Vec::pop) {
                    bars.push((*who, from, at));
                }
            }
            _ => {}
        }
    }
    for (who, starts) in open {
        for from in starts {
            bars.push((who, from, frame.bottom));
        }
    }
    bars.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    bars
}

/// Draw each participant's box, at the top and again at the bottom.
fn draw_heads(
    scene: &mut Scene,
    diagram: &Diagram,
    labels: &[Label],
    frame: &Frame,
    options: &Options,
) {
    let style = parts::text_style(options, 0.95, true, options.theme.text);
    for (index, who) in diagram.participants.iter().enumerate() {
        for (at_the_top, top) in [(true, frame.top), (false, frame.bottom + 6.0)] {
            let rect = Rect::new(
                frame.heads[index].left(),
                top,
                frame.heads[index].width,
                HEAD_HEIGHT,
            );
            if who.is_actor {
                // The figure goes above the name at the top of the diagram and below it at the
                // bottom, so it is always on the outside and never lands inside the last block frame.
                let figure = if at_the_top { rect.y - HEAD_HEIGHT } else { rect.bottom() };
                draw_actor(scene, Rect::new(rect.x, figure, rect.width, HEAD_HEIGHT), options);
            } else {
                scene.add(Item::Rect {
                    rect,
                    radius: parts::CORNER,
                    fill: Some(options.theme.node_fill),
                    stroke: Some(Stroke::new(options.theme.node_stroke, parts::LINE)),
                });
            }
            parts::centred_label(scene, &labels[index], rect, &style);
        }
    }
}

/// An actor: a stick figure above the name, so it reads as a person rather than as a system.
///
/// Drawn in the line colour rather than in the border colour. A border is meant to be quiet against
/// the fill it surrounds; a stick figure with nothing behind it and no fill of its own has only its
/// own strokes, and in the border colour it was very nearly invisible against the editing area.
fn draw_actor(scene: &mut Scene, rect: Rect, options: &Options) {
    let stroke = Stroke::new(options.theme.line, parts::LINE);
    let centre = rect.centre();
    let head = rect.height * 0.16;
    scene.add(Item::Circle {
        centre: Point::new(centre.x, rect.top() + head + 2.0),
        radius: head,
        fill: Some(options.theme.node_fill),
        stroke: Some(stroke),
    });
    let shoulder = rect.top() + head * 2.0 + 2.0;
    let foot = rect.bottom() - 2.0;
    let arm = rect.height * 0.22;
    for points in [
        vec![Point::new(centre.x, shoulder), Point::new(centre.x, foot - arm)],
        vec![Point::new(centre.x - arm, shoulder + arm * 0.4), Point::new(centre.x + arm, shoulder + arm * 0.4)],
        vec![Point::new(centre.x - arm, foot), Point::new(centre.x, foot - arm)],
        vec![Point::new(centre.x + arm, foot), Point::new(centre.x, foot - arm)],
    ] {
        scene.add(Item::Line { points, stroke, dash: Dash::Solid });
    }
}

/// Draw the messages, the notes and the block frames.
fn draw_events(
    scene: &mut Scene,
    diagram: &Diagram,
    labels: &[Label],
    frame: &Frame,
    options: &Options,
) {
    draw_blocks(scene, diagram, labels, frame, options);
    let mut number = 0;
    for (index, event) in diagram.events.iter().enumerate() {
        let y = frame.rows[index];
        match event {
            Event::Message { from, to, arrow, text } => {
                number += 1;
                let numbered = diagram.numbered.then_some(number);
                draw_message(scene, *from, *to, *arrow, text, &labels[index], y, numbered, frame, options);
            }
            Event::Note { over, side, text } => {
                let _ = text;
                draw_note(scene, over, *side, &labels[index], y, frame, options);
            }
            _ => {}
        }
    }
}

/// Draw one message: its line, its head, and its words above it.
#[allow(clippy::too_many_arguments)]
fn draw_message(
    scene: &mut Scene,
    from: usize,
    to: usize,
    arrow: Arrow,
    text: &str,
    label: &Label,
    y: f32,
    number: Option<usize>,
    frame: &Frame,
    options: &Options,
) {
    let theme = &options.theme;
    let stroke = Stroke::new(theme.line, parts::LINE);
    let dash = if arrow.dotted { parts::DASH } else { Dash::Solid };
    let (start, finish) = (frame.columns[from], frame.columns[to]);
    let path = if from == to {
        // A message to oneself loops out to the right and back, which is what Mermaid draws.
        let out = 34.0;
        vec![
            Point::new(start + BAR / 2.0, y - 10.0),
            Point::new(start + out, y - 10.0),
            Point::new(start + out, y + 10.0),
            Point::new(start + BAR / 2.0, y + 10.0),
        ]
    } else {
        let step = if finish > start { BAR / 2.0 } else { -BAR / 2.0 };
        vec![Point::new(start + step, y), Point::new(finish - step, y)]
    };
    let trimmed = parts::trimmed(&path, 0.0, parts::ending_inset(arrow.head));
    scene.add(Item::Line { points: trimmed, stroke, dash });
    parts::ending(
        scene,
        arrow.head,
        path[path.len() - 1],
        parts::heading(&path),
        theme.line,
        theme.node_fill,
    );
    if arrow.both {
        parts::ending(scene, arrow.head, path[0], parts::tail_heading(&path), theme.line, theme.node_fill);
    }
    if label.is_empty() && number.is_none() {
        return;
    }
    let words = match number {
        Some(count) => format!("{count}. {text}"),
        None => text.to_owned(),
    };
    let middle = if from == to {
        Point::new(start + 44.0, y - label.height - 4.0)
    } else {
        Point::new((start + finish) / 2.0, y - label.height - 6.0)
    };
    let style = parts::text_style(options, 0.85, false, theme.text);
    let width = text::width_of(&words, &options.style(0.85, false), options.metrics);
    let anchor = if from == to { Anchor::Start } else { Anchor::Middle };
    parts::one_line(scene, &words, middle, &style, anchor, width);
}

/// Draw a note as a small panel beside or over the participants it names.
#[allow(clippy::too_many_arguments)]
fn draw_note(
    scene: &mut Scene,
    over: &[usize],
    side: Side,
    label: &Label,
    y: f32,
    frame: &Frame,
    options: &Options,
) {
    let width = label.width + parts::PADDING_X * 2.0;
    let height = label.height + 12.0;
    let first = *over.first().expect("a note names at least one participant");
    let last = *over.last().expect("a note names at least one participant");
    let rect = match side {
        Side::Left => Rect::new(frame.columns[first] - width - 24.0, y - height / 2.0, width, height),
        Side::Right => Rect::new(frame.columns[first] + 24.0, y - height / 2.0, width, height),
        Side::Over => {
            let left = frame.columns[first].min(frame.columns[last]);
            let right = frame.columns[first].max(frame.columns[last]);
            let across = (right - left + width).max(width);
            Rect::new((left + right) / 2.0 - across / 2.0, y - height / 2.0, across, height)
        }
    };
    scene.add(Item::Rect {
        rect,
        radius: parts::CORNER,
        fill: Some(Paint::solid(options.theme.node_fill.color)),
        stroke: Some(Stroke::new(options.theme.node_stroke, parts::LINE)),
    });
    parts::centred_label(
        scene,
        label,
        rect,
        &parts::text_style(options, 0.85, false, options.theme.text),
    );
}

/// Draw the frames round `loop`, `alt`, `opt` and the rest.
fn draw_blocks(
    scene: &mut Scene,
    diagram: &Diagram,
    labels: &[Label],
    frame: &Frame,
    options: &Options,
) {
    let theme = &options.theme;
    let mut open: Vec<(usize, String, String, Vec<usize>)> = Vec::new();
    for (index, event) in diagram.events.iter().enumerate() {
        match event {
            Event::Open { kind, label } => {
                open.push((index, kind.clone(), label.clone(), Vec::new()));
            }
            Event::Divide { .. } => {
                if let Some(last) = open.last_mut() {
                    last.3.push(index);
                }
            }
            Event::Close => {
                let Some((start, kind, label, divides)) = open.pop() else {
                    continue;
                };
                let rect = Rect::new(
                    frame.columns.first().copied().unwrap_or(0.0) - FRAME_INSET - 10.0,
                    frame.rows[start] - FRAME_HEADER / 2.0 - 6.0,
                    frame.columns.last().copied().unwrap_or(0.0)
                        - frame.columns.first().copied().unwrap_or(0.0)
                        + (FRAME_INSET + 10.0) * 2.0,
                    frame.rows[index] - frame.rows[start] + FRAME_HEADER,
                );
                scene.add(Item::Rect {
                    rect,
                    radius: parts::CORNER,
                    fill: None,
                    stroke: Some(Stroke::new(theme.group_stroke, parts::LINE)),
                });
                draw_block_tab(scene, &kind, &label, &labels[start], rect, options);
                for divide in divides {
                    let y = frame.rows[divide] - FRAME_HEADER / 2.0;
                    scene.add(Item::Line {
                        points: vec![Point::new(rect.left(), y), Point::new(rect.right(), y)],
                        stroke: Stroke::new(theme.group_stroke, parts::LINE),
                        dash: parts::DASH,
                    });
                    let words = match &diagram.events[divide] {
                        Event::Divide { label } => label.clone(),
                        _ => String::new(),
                    };
                    let style = parts::text_style(options, 0.8, false, theme.dim);
                    let width =
                        text::width_of(&words, &options.style(0.8, false), options.metrics);
                    parts::one_line(
                        scene,
                        &words,
                        Point::new(rect.left() + 12.0, y + 3.0),
                        &style,
                        Anchor::Start,
                        width,
                    );
                }
            }
            _ => {}
        }
    }
}

/// The little tab in a block's top left corner saying what kind of block it is.
fn draw_block_tab(
    scene: &mut Scene,
    kind: &str,
    label: &str,
    measured: &Label,
    rect: Rect,
    options: &Options,
) {
    let style = options.style(0.8, true);
    let word_width = text::width_of(kind, &style, options.metrics);
    let tab = Rect::new(rect.left(), rect.top(), word_width + 16.0, FRAME_HEADER - 6.0);
    scene.add(Item::Rect {
        rect: tab,
        radius: parts::CORNER,
        fill: Some(Paint::solid(options.theme.node_fill.color)),
        stroke: Some(Stroke::new(options.theme.group_stroke, parts::LINE)),
    });
    parts::one_line(
        scene,
        kind,
        Point::new(tab.centre().x, tab.top() + 3.0),
        &parts::text_style(options, 0.8, true, options.theme.text),
        Anchor::Middle,
        word_width,
    );
    if label.trim().is_empty() {
        return;
    }
    parts::one_line(
        scene,
        label,
        Point::new(tab.right() + 10.0, tab.top() + 3.0),
        &parts::text_style(options, 0.8, false, options.theme.dim),
        Anchor::Start,
        measured.width,
    );
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
    fn participants_come_in_the_order_they_are_declared() {
        let diagram = diagram("sequenceDiagram\n participant B as Bob\n participant A as Alice\n A ->> B: Hi\n");
        assert_eq!(diagram.participants.len(), 2);
        assert_eq!(diagram.participants[0].label, "Bob");
        assert_eq!(diagram.participants[1].label, "Alice");
    }

    #[test]
    fn a_participant_that_is_never_declared_still_gets_a_column() {
        let diagram = diagram("sequenceDiagram\n Alice ->> Bob: Hello\n Bob -->> Alice: Hi\n");
        assert_eq!(diagram.participants.len(), 2);
        assert_eq!(diagram.participants[0].label, "Alice");
    }

    #[test]
    fn every_arrow_form_is_read() {
        let diagram = diagram(
            "sequenceDiagram\n A -> B: one\n A --> B: two\n A ->> B: three\n A -->> B: four\n \
             A -x B: five\n A --x B: six\n A -) B: seven\n A <<->> B: eight\n",
        );
        let arrows: Vec<Arrow> = diagram
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Message { arrow, .. } => Some(*arrow),
                _ => None,
            })
            .collect();
        assert_eq!(arrows.len(), 8);
        assert!(!arrows[0].dotted && arrows[0].head == Ending::None);
        assert!(arrows[1].dotted);
        assert_eq!(arrows[2].head, Ending::Arrow);
        assert!(arrows[3].dotted && arrows[3].head == Ending::Arrow);
        assert_eq!(arrows[4].head, Ending::Cross);
        assert!(arrows[5].dotted && arrows[5].head == Ending::Cross);
        assert!(arrows[7].both, "<<->> points both ways");
    }

    #[test]
    fn the_plus_and_minus_shorthand_activates_and_deactivates() {
        let diagram = diagram("sequenceDiagram\n A ->>+ B: ask\n B -->>- A: answer\n");
        let kinds: Vec<&str> = diagram
            .events
            .iter()
            .map(|event| match event {
                Event::Message { .. } => "message",
                Event::Activate(_) => "activate",
                Event::Deactivate(_) => "deactivate",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["message", "activate", "deactivate", "message"]);
    }

    #[test]
    fn notes_are_read_on_all_three_sides() {
        let diagram = diagram(
            "sequenceDiagram\n A ->> B: hi\n Note left of A: thinking\n \
             Note right of B: waiting\n Note over A,B: together\n",
        );
        let notes: Vec<(Side, usize)> = diagram
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Note { side, over, .. } => Some((*side, over.len())),
                _ => None,
            })
            .collect();
        assert_eq!(notes, vec![(Side::Left, 1), (Side::Right, 1), (Side::Over, 2)]);
    }

    #[test]
    fn every_block_word_opens_a_block_that_end_closes() {
        let text = "sequenceDiagram\n\
            loop every day\n A ->> B: check\n end\n\
            alt is it ready\n A ->> B: ship\n else it is not\n A ->> B: wait\n end\n\
            opt maybe\n A ->> B: extra\n end\n\
            par one\n A ->> B: a\n and two\n A ->> B: b\n end\n\
            critical must work\n A ->> B: c\n option it did not\n A ->> B: d\n end\n\
            break gave up\n A ->> B: stop\n end\n";
        let diagram = diagram(text);
        let opens = diagram.events.iter().filter(|e| matches!(e, Event::Open { .. })).count();
        let closes = diagram.events.iter().filter(|e| matches!(e, Event::Close)).count();
        let divides = diagram.events.iter().filter(|e| matches!(e, Event::Divide { .. })).count();
        assert_eq!(opens, 6);
        assert_eq!(closes, 6);
        assert_eq!(divides, 3, "else, and, option");
    }

    #[test]
    fn a_box_groups_participants_and_its_end_does_not_close_a_block() {
        let text = "sequenceDiagram\n box Aqua The Team\n participant A\n participant B\n end\n \
                    participant C\n A ->> C: hello\n";
        let diagram = diagram(text);
        assert_eq!(diagram.bands, vec!["The Team"]);
        assert_eq!(diagram.participants[0].band, Some(0));
        assert_eq!(diagram.participants[2].band, None, "C is outside the box");
        assert!(
            !diagram.events.iter().any(|e| matches!(e, Event::Close)),
            "the box's end must not close a block that was never opened"
        );
    }

    #[test]
    fn a_line_that_is_not_a_message_says_so_with_its_line_number() {
        let problem = check::refused("sequenceDiagram\n A ->> B: fine\n what is this\n", &options());
        assert_eq!(problem.line, Some(3));
        assert!(problem.reason.contains("Alice ->> Bob"), "it says what one looks like");
    }

    #[test]
    fn a_sequence_diagram_is_drawn_and_keeps_every_property() {
        let text = "sequenceDiagram\n\
            autonumber\n\
            actor Alice\n participant Bob as The Server\n\
            Alice ->>+ Bob: Can I have it?\n\
            Note right of Bob: looking it up\n\
            loop until found\n  Bob ->> Bob: search\n end\n\
            alt it was there\n  Bob -->>- Alice: Here you are\n else it was not\n  Bob --x Alice: Sorry\n end\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Alice", "The Server", "Can I have it?", "looking it up", "loop", "until found", "alt"],
        );
        assert!(scene.size.height > scene.size.width / 2.0, "a sequence diagram runs downwards");
    }

    #[test]
    fn the_columns_are_far_enough_apart_for_the_words_between_them() {
        // A long message must not run out over the participant beside it.
        let text = "sequenceDiagram\n A ->> B: a very long message indeed that needs plenty of room\n";
        let scene = check::drawn(text, &options(), &["a very long message"]);
        assert!(scene.size.width > 400.0, "the columns were pushed apart: {:?}", scene.size);
    }

    #[test]
    fn a_message_to_oneself_loops_rather_than_disappearing() {
        let scene = check::drawn("sequenceDiagram\n A ->> A: think\n", &options(), &["think"]);
        assert!(scene.items.iter().any(|item| matches!(item, Item::Line { points, .. } if points.len() > 2)));
    }

    #[test]
    fn an_empty_sequence_diagram_draws_nothing_rather_than_failing() {
        let scene = super::super::render("sequenceDiagram\n", &options()).expect("it should draw");
        assert!(scene.is_empty());
    }
}

#[cfg(test)]
mod actors {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    #[test]
    fn an_actors_figure_is_drawn_outside_the_diagram_at_both_ends() {
        // Above its name at the top and below it at the bottom, so it is always on the outside. Drawn
        // above at both ends, the one at the bottom lands inside the last block frame.
        let text = "sequenceDiagram\n actor A\n participant B\n loop forever\n A ->> B: hello\n end\n";
        let scene = check::drawn(text, &options(), &["A", "B", "hello", "loop"]);
        let names: Vec<f32> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text { at, text, .. } if text == "A" => Some(at.y),
                _ => None,
            })
            .collect();
        let heads: Vec<f32> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Circle { centre, radius, .. } if *radius < HEAD_HEIGHT / 2.0 => Some(centre.y),
                _ => None,
            })
            .collect();
        assert_eq!(names.len(), 2, "the name is drawn at the top and at the bottom");
        assert_eq!(heads.len(), 2, "and so is the figure's head");
        assert!(heads[0] < names[0], "the top figure is above its name");
        assert!(heads[1] > names[1], "the bottom figure is below its name");
    }
}
