//! What the lines of a document are, before anything is decided about how they look.
//!
//! The old preview classified each line on its own, which is why a list item holding a paragraph, a
//! quote holding a list, and a fence indented inside a bullet all came out as unrelated lines at the
//! left margin. Markdown is a tree, so this builds one.
//!
//! The parser is recursive rather than incremental: a quote's lines have one `>` taken off them and
//! are parsed again, and a list item's lines have its indent taken off them and are parsed again.
//! That is a different shape from CommonMark's reference implementation, which keeps a stack of open
//! containers and walks the document once — and it gives the same answers for everything short of
//! the pathological cases, at a fraction of the reading. A preview is worked out again on every
//! keystroke, so being easy to follow is worth more here than being able to parse a document
//! nobody would write.
//!
//! Two rules are worth naming because everything else falls out of them.
//!
//! **Lazy continuation.** A plain line under a paragraph continues that paragraph even when the
//! quote's `>` or the item's indent is missing. Nearly every Markdown file anybody has hand-wrapped
//! depends on it, and without it every wrapped line was its own paragraph.
//!
//! **Tight and loose.** A list is loose when a blank line separates any two of its items or splits
//! one item's own content. A tight list is drawn with its items one under the other and a loose one
//! with air between them, which is the whole of the difference and is why lists used to look so
//! spread out: every item was followed by a blank line whatever the source said.

use super::inline::References;
use super::table::{self, Table};

/// One line of the source, carrying where it came from so the preview can be scrolled against it.
#[derive(Debug, Clone)]
pub(crate) struct Line {
    pub number: usize,
    pub text: String,
}

/// A block, and the line of the source it started on.
#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub line: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Heading { level: usize, content: String },
    Paragraph { content: String },
    Quote(Vec<Block>),
    List(List),
    /// A fenced or indented code block, one entry a line so each keeps its own source line.
    Code { language: String, lines: Vec<Line> },
    /// A `mermaid` fence, kept whole for the window to draw.
    Diagram { source: String },
    Table(Table),
    Rule,
    /// The YAML block at the top of a file written for a static site.
    FrontMatter(Vec<Line>),
    /// A picture on a line of its own.
    Image { source: String, alt: String },
    /// A footnote's own text, which is drawn where it was written.
    Footnote { number: usize, blocks: Vec<Block> },
}

#[derive(Debug, Clone)]
pub(crate) struct List {
    pub ordered: bool,
    /// What the first item is numbered, so a list starting at three starts at three.
    pub start: u64,
    /// Whether the items are drawn one under the other, or with air between them.
    pub tight: bool,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub line: usize,
    /// `Some(true)` for a ticked box, `Some(false)` for an empty one, `None` for an ordinary item.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// Read a whole document.
pub(crate) fn parse(source: &str) -> (Vec<Block>, References) {
    let mut lines: Vec<Line> = source
        .split('\n')
        .enumerate()
        .map(|(number, text)| Line { number, text: text.trim_end_matches('\r').to_owned() })
        .collect();

    let mut blocks = Vec::new();
    // Front matter is only front matter at the very front, which is what stops a rule three lines
    // into a document from swallowing everything under it.
    if let Some(end) = front_matter_end(&lines) {
        blocks.push(Block { line: 0, kind: Kind::FrontMatter(lines[1..end].to_vec()) });
        lines.drain(..=end);
    }

    let mut references = References::default();
    blocks.extend(parse_blocks(&lines, &mut references));
    (blocks, references)
}

/// Where the closing `---` of a file's front matter is, if it has any.
fn front_matter_end(lines: &[Line]) -> Option<usize> {
    if lines.first().map(|line| line.text.trim_end()) != Some("---") {
        return None;
    }
    lines[1..]
        .iter()
        .position(|line| matches!(line.text.trim_end(), "---" | "..."))
        .map(|at| at + 1)
}

/// Read a run of lines that have already had every container's prefix taken off them.
fn parse_blocks(lines: &[Line], references: &mut References) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = &lines[at];
        if is_blank(&line.text) {
            at += 1;
            continue;
        }
        if let Some((fence, language, indent)) = fence_open(&line.text) {
            at += 1;
            let start = at;
            while at < lines.len() && !fence_closes(&lines[at].text, fence) {
                at += 1;
            }
            let body: Vec<Line> = lines[start..at]
                .iter()
                .map(|line| Line { number: line.number, text: strip_indent(&line.text, indent) })
                .collect();
            if at < lines.len() {
                at += 1;
            }
            blocks.push(fenced_block(line.number, &language, body));
            continue;
        }
        if let Some((level, content)) = atx_heading(&line.text) {
            blocks.push(Block { line: line.number, kind: Kind::Heading { level, content } });
            at += 1;
            continue;
        }
        if is_thematic_break(&line.text) {
            blocks.push(Block { line: line.number, kind: Kind::Rule });
            at += 1;
            continue;
        }
        if let Some((source, alt)) = whole_line_image(line.text.trim()) {
            blocks.push(Block {
                line: line.number,
                kind: Kind::Image { source: source.to_owned(), alt: alt.to_owned() },
            });
            at += 1;
            continue;
        }
        if quote_prefix(&line.text).is_some() {
            let (inner, used) = gather_quote(lines, at);
            blocks.push(Block {
                line: line.number,
                kind: Kind::Quote(parse_blocks(&inner, references)),
            });
            at = used;
            continue;
        }
        if let Some((label, name)) = footnote_definition(&line.text) {
            let (inner, used) = gather_footnote(lines, at, &name);
            let number = footnote_number(references, &label);
            blocks.push(Block {
                line: line.number,
                kind: Kind::Footnote { number, blocks: parse_blocks(&inner, references) },
            });
            at = used;
            continue;
        }
        if marker(&line.text).is_some() {
            let (list, used) = gather_list(lines, at, references);
            blocks.push(Block { line: line.number, kind: Kind::List(list) });
            at = used;
            continue;
        }
        if indent_of(&line.text) >= 4 {
            let start = at;
            let mut last = at;
            while at < lines.len() && (is_blank(&lines[at].text) || indent_of(&lines[at].text) >= 4) {
                if !is_blank(&lines[at].text) {
                    last = at;
                }
                at += 1;
            }
            let body: Vec<Line> = lines[start..=last]
                .iter()
                .map(|line| Line { number: line.number, text: strip_indent(&line.text, 4) })
                .collect();
            blocks.push(Block {
                line: line.number,
                kind: Kind::Code { language: String::new(), lines: body },
            });
            at = last + 1;
            continue;
        }
        if let Some((parsed, used)) = table::read(lines, at) {
            blocks.push(Block { line: line.number, kind: Kind::Table(parsed) });
            at = used;
            continue;
        }
        if is_html_block(&line.text) {
            while at < lines.len() && !is_blank(&lines[at].text) {
                at += 1;
            }
            continue;
        }
        if let Some((label, destination, used)) = link_definition(lines, at) {
            references.links.push((References::normalise(&label), destination));
            at = used;
            continue;
        }
        let (block, used) = gather_paragraph(lines, at, references);
        if let Some(block) = block {
            blocks.push(block);
        }
        at = used;
    }
    blocks
}

/// A fence is a code block unless it says `mermaid`, which is a diagram the window draws.
fn fenced_block(line: usize, language: &str, body: Vec<Line>) -> Block {
    if language.split_whitespace().next().is_some_and(|word| word.eq_ignore_ascii_case("mermaid")) {
        let source: Vec<&str> = body.iter().map(|line| line.text.as_str()).collect();
        // Trailing blank lines are dropped, because a fence nobody has closed yet collects the empty
        // line at the end of the file, and a diagram should not differ depending on whether its
        // author has finished typing the closing backticks.
        return Block { line, kind: Kind::Diagram { source: source.join("\n").trim_end().to_owned() } };
    }
    Block { line, kind: Kind::Code { language: language.to_owned(), lines: body } }
}

/// Gather the lines of a block quote, taking one `>` and one optional space off each.
///
/// A plain line is taken too when the line before it was prose, which is lazy continuation: a
/// wrapped quote is usually written with the `>` only on the first line.
fn gather_quote(lines: &[Line], mut at: usize) -> (Vec<Line>, usize) {
    let mut inner = Vec::new();
    let mut previous_was_prose = false;
    while at < lines.len() {
        if let Some(rest) = quote_prefix(&lines[at].text) {
            previous_was_prose = !is_blank(rest) && !starts_a_block(rest);
            inner.push(Line { number: lines[at].number, text: rest.to_owned() });
            at += 1;
            continue;
        }
        let text = &lines[at].text;
        if previous_was_prose && !is_blank(text) && !starts_a_block(text) {
            inner.push(lines[at].clone());
            at += 1;
            continue;
        }
        break;
    }
    (inner, at)
}

/// The number a footnote label is drawn as, added to the list the first time it is seen.
fn footnote_number(references: &mut References, label: &str) -> usize {
    let wanted = References::normalise(label);
    if let Some(at) = references.footnotes.iter().position(|name| *name == wanted) {
        return at + 1;
    }
    references.footnotes.push(wanted);
    references.footnotes.len()
}

/// Gather a footnote's own text: the rest of its first line, plus every indented line under it.
fn gather_footnote(lines: &[Line], at: usize, first: &str) -> (Vec<Line>, usize) {
    let mut inner = vec![Line { number: lines[at].number, text: first.to_owned() }];
    let mut next = at + 1;
    while next < lines.len() {
        let text = &lines[next].text;
        if is_blank(text) {
            // A blank line ends the note unless indented text follows it.
            let more = lines[next + 1..]
                .iter()
                .find(|line| !is_blank(&line.text))
                .is_some_and(|line| indent_of(&line.text) >= 4);
            if !more {
                break;
            }
            inner.push(Line { number: lines[next].number, text: String::new() });
            next += 1;
            continue;
        }
        if indent_of(text) < 4 {
            break;
        }
        inner.push(Line { number: lines[next].number, text: strip_indent(text, 4) });
        next += 1;
    }
    (inner, next)
}

/// What a list item's marker is.
#[derive(Debug, Clone, Copy)]
struct Marker {
    ordered: bool,
    number: u64,
    /// How many spaces are in front of the marker.
    indent: usize,
    /// How far in the item's own content starts.
    content: usize,
    /// Where the content starts in the line, in bytes.
    at: usize,
}

/// Read a list marker, which is a bullet or a number followed by a space or the end of the line.
fn marker(text: &str) -> Option<Marker> {
    let indent = indent_of(text);
    if indent >= 4 {
        return None;
    }
    // A rule beats a list, so `- - -` is a rule rather than a bullet holding one.
    if is_thematic_break(text) {
        return None;
    }
    let body = &text[text.len() - text.trim_start().len()..];
    for bullet in ['-', '*', '+'] {
        if let Some(rest) = body.strip_prefix(bullet) {
            if rest.is_empty() || rest.starts_with(' ') {
                let spaces = rest.len() - rest.trim_start().len();
                let width = if rest.trim().is_empty() { 2 } else { 1 + spaces.min(4) };
                return Some(Marker {
                    ordered: false,
                    number: 0,
                    indent,
                    content: indent + width,
                    at: indent + width.min(1 + rest.len()),
                });
            }
        }
    }
    let digits: String = body.chars().take_while(char::is_ascii_digit).take(9).collect();
    if digits.is_empty() {
        return None;
    }
    let after = &body[digits.len()..];
    for delimiter in ['.', ')'] {
        if let Some(rest) = after.strip_prefix(delimiter) {
            if rest.is_empty() || rest.starts_with(' ') {
                let spaces = rest.len() - rest.trim_start().len();
                let width =
                    if rest.trim().is_empty() { digits.len() + 2 } else { digits.len() + 1 + spaces.min(4) };
                return Some(Marker {
                    ordered: true,
                    number: digits.parse().unwrap_or(1),
                    indent,
                    content: indent + width,
                    at: indent + width.min(digits.len() + 1 + rest.len()),
                });
            }
        }
    }
    None
}

/// Gather a whole list, one item at a time, and decide whether it is tight.
fn gather_list(lines: &[Line], mut at: usize, references: &mut References) -> (List, usize) {
    let first = marker(&lines[at].text).expect("only called on a marker");
    let ordered = first.ordered;
    let start = first.number;
    let mut items = Vec::new();
    let mut loose = false;

    // Whether the line at `index` starts another item of *this* list, rather than of a different
    // one. Asked twice: to keep gathering, and to decide whether the blank line before it is a blank
    // inside a loose list or the gap between two lists — which is not the same question, and
    // answering it with "is there a marker here" made a bullet list followed by a numbered one loose
    // for no reason a reader could see.
    let continues = |index: usize| -> bool {
        lines
            .get(index)
            .and_then(|line| marker(&line.text))
            .is_some_and(|next| next.ordered == ordered && next.indent <= first.indent + 3)
    };

    while at < lines.len() {
        if !continues(at) {
            break;
        }
        let this = marker(&lines[at].text).expect("continues said there is one");
        let content = this.content;
        let mut inner =
            vec![Line { number: lines[at].number, text: lines[at].text[this.at.min(lines[at].text.len())..].to_owned() }];
        at += 1;
        let mut blanks = 0;
        let mut prose = !is_blank(&inner[0].text);
        while at < lines.len() {
            let text = &lines[at].text;
            if is_blank(text) {
                blanks += 1;
                inner.push(Line { number: lines[at].number, text: String::new() });
                at += 1;
                continue;
            }
            if indent_of(text) >= content {
                if blanks > 0 {
                    loose = true;
                }
                blanks = 0;
                prose = !starts_a_block(&strip_indent(text, content));
                inner.push(Line { number: lines[at].number, text: strip_indent(text, content) });
                at += 1;
                continue;
            }
            // A plain line under prose continues it even without the item's indent.
            if blanks == 0 && prose && marker(text).is_none() && !starts_a_block(text) {
                inner.push(lines[at].clone());
                at += 1;
                continue;
            }
            break;
        }
        // The blank lines at the end of an item belong to the list, not to the item — but they are
        // what makes it loose when another item follows.
        while inner.last().is_some_and(|line| is_blank(&line.text)) {
            inner.pop();
        }
        if blanks > 0 && continues(at) {
            loose = true;
        }
        let (task, inner) = read_task(inner);
        items.push(Item {
            line: inner.first().map(|line| line.number).unwrap_or(0),
            task,
            blocks: parse_blocks(&inner, references),
        });
    }
    (List { ordered, start, tight: !loose, items }, at)
}

/// Take a task list's tick box off the front of an item, if it has one.
fn read_task(mut inner: Vec<Line>) -> (Option<bool>, Vec<Line>) {
    let Some(first) = inner.first_mut() else { return (None, inner) };
    let text = first.text.trim_start().to_owned();
    let done = match text.strip_prefix("[ ]") {
        Some(rest) => Some((false, rest.to_owned())),
        None => text
            .strip_prefix("[x]")
            .or_else(|| text.strip_prefix("[X]"))
            .map(|rest| (true, rest.to_owned())),
    };
    match done {
        Some((ticked, rest)) if rest.is_empty() || rest.starts_with(' ') => {
            first.text = rest.trim_start().to_owned();
            (Some(ticked), inner)
        }
        _ => (None, inner),
    }
}

/// Gather a paragraph, stopping at a blank line or at anything that starts a block of its own.
///
/// A run of `=` or `-` under it makes it a heading instead, which is the older way of writing one
/// and is still very common in files that have been around a while.
fn gather_paragraph(
    lines: &[Line],
    mut at: usize,
    references: &mut References,
) -> (Option<Block>, usize) {
    let start = lines[at].number;
    let mut content: Vec<&str> = Vec::new();
    while at < lines.len() {
        let text = &lines[at].text;
        if is_blank(text) {
            break;
        }
        if !content.is_empty() {
            if let Some(level) = setext_underline(text) {
                let heading = Kind::Heading { level, content: content.join("\n") };
                return (Some(Block { line: start, kind: heading }), at + 1);
            }
            if starts_a_block(text) || table::starts_here(lines, at) {
                break;
            }
        }
        content.push(text);
        at += 1;
    }
    if content.is_empty() {
        return (None, at + 1);
    }
    // A paragraph that is nothing but link definitions produces no paragraph at all.
    let mut kept: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < content.len() {
        if let Some((label, destination, used)) = definition_in(&content, index) {
            if kept.is_empty() {
                references.links.push((References::normalise(label), destination));
                index = used;
                continue;
            }
        }
        kept.push(content[index]);
        index += 1;
    }
    if kept.is_empty() {
        return (None, at);
    }
    (Some(Block { line: start, kind: Kind::Paragraph { content: kept.join("\n") } }), at)
}

/// True for a line that begins something other than more of the paragraph above it.
fn starts_a_block(text: &str) -> bool {
    is_thematic_break(text)
        || atx_heading(text).is_some()
        || fence_open(text).is_some()
        || quote_prefix(text).is_some()
        || is_html_block(text)
        || marker(text).is_some_and(|found| can_interrupt(text, found))
}

/// A list may interrupt a paragraph only when it has something in it, and an ordered one only when
/// it starts at one — otherwise `the year 2015. It was` would become a list.
fn can_interrupt(text: &str, found: Marker) -> bool {
    if text[found.at.min(text.len())..].trim().is_empty() {
        return false;
    }
    !found.ordered || found.number == 1
}

/// A link definition, read across as many lines as it takes.
fn link_definition(lines: &[Line], at: usize) -> Option<(String, String, usize)> {
    let window: Vec<&str> = lines[at..].iter().map(|line| line.text.as_str()).collect();
    let (label, destination, used) = definition_in(&window, 0)?;
    Some((label.to_owned(), destination, at + used))
}

/// Read `[label]: destination "title"` starting at `index`, which may run onto the next line.
fn definition_in<'a>(lines: &[&'a str], index: usize) -> Option<(&'a str, String, usize)> {
    let text = lines.get(index)?;
    let trimmed = text.trim_start();
    if indent_of(text) >= 4 || !trimmed.starts_with('[') || trimmed.starts_with("[^") {
        return None;
    }
    let close = trimmed.find("]:")?;
    let label = &trimmed[1..close];
    if label.is_empty() {
        return None;
    }
    let rest = trimmed[close + 2..].trim();
    let (destination, used) = if rest.is_empty() {
        // The destination may be on the line under the label.
        let next = lines.get(index + 1)?.trim();
        if next.is_empty() {
            return None;
        }
        (next.split_whitespace().next()?, index + 2)
    } else {
        (rest.split_whitespace().next()?, index + 1)
    };
    let destination = destination.trim_matches(|c| c == '<' || c == '>');
    Some((label, destination.to_owned(), used))
}

/// A footnote definition: its label and whatever was written after the colon.
fn footnote_definition(text: &str) -> Option<(String, String)> {
    if indent_of(text) >= 4 {
        return None;
    }
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() {
        return None;
    }
    Some((label.to_owned(), rest[close + 2..].trim_start().to_owned()))
}

/// How many columns of indent a line has, counting a tab as four.
pub(crate) fn indent_of(text: &str) -> usize {
    let mut columns = 0;
    for character in text.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4 - columns % 4,
            _ => break,
        }
    }
    columns
}

/// Take `columns` of indent off a line, keeping whatever is left of a tab that straddles the edge.
pub(crate) fn strip_indent(text: &str, columns: usize) -> String {
    let mut taken = 0;
    let mut at = 0;
    for character in text.chars() {
        if taken >= columns {
            break;
        }
        match character {
            ' ' => taken += 1,
            '\t' => taken += 4 - taken % 4,
            _ => break,
        }
        at += character.len_utf8();
    }
    let mut out = String::new();
    if taken > columns {
        out.push_str(&" ".repeat(taken - columns));
    }
    out.push_str(&text[at..]);
    out
}

pub(crate) fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Three or more of `-`, `*` or `_`, with any amount of space between them and nothing else.
fn is_thematic_break(text: &str) -> bool {
    if indent_of(text) >= 4 {
        return false;
    }
    let body = text.trim();
    for mark in ['-', '*', '_'] {
        let count = body.chars().filter(|c| *c == mark).count();
        if count >= 3 && body.chars().all(|c| c == mark || c == ' ' || c == '\t') {
            return true;
        }
    }
    false
}

/// A heading written with hashes, with its closing hashes taken off.
fn atx_heading(text: &str) -> Option<(usize, String)> {
    if indent_of(text) >= 4 {
        return None;
    }
    let body = text.trim();
    let level = body.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &body[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let mut content = rest.trim();
    // A closing run of hashes is decoration and is not shown.
    if content.ends_with('#') {
        let trimmed = content.trim_end_matches('#');
        if trimmed.is_empty() || trimmed.ends_with(' ') {
            content = trimmed.trim_end();
        }
    }
    Some((level, content.to_owned()))
}

/// A run of `=` or `-` under a paragraph, which makes it a heading.
fn setext_underline(text: &str) -> Option<usize> {
    if indent_of(text) >= 4 {
        return None;
    }
    let body = text.trim();
    if body.is_empty() {
        return None;
    }
    if body.chars().all(|c| c == '=') {
        return Some(1);
    }
    if body.chars().all(|c| c == '-') {
        return Some(2);
    }
    None
}

/// A fence, with the character it is made of, the language after it and how far it is indented.
fn fence_open(text: &str) -> Option<(char, String, usize)> {
    let indent = indent_of(text);
    if indent >= 4 {
        return None;
    }
    let body = text.trim_start();
    for mark in ['`', '~'] {
        let count = body.chars().take_while(|c| *c == mark).count();
        if count >= 3 {
            let rest = body[count..].trim();
            // A backtick fence's language cannot itself hold a backtick.
            if mark == '`' && rest.contains('`') {
                return None;
            }
            return Some((mark, rest.to_owned(), indent));
        }
    }
    None
}

/// True for the line that closes a fence made of `mark`.
fn fence_closes(text: &str, mark: char) -> bool {
    if indent_of(text) >= 4 {
        return false;
    }
    let body = text.trim();
    body.len() >= 3 && body.chars().all(|c| c == mark)
}

/// A line that starts a block of HTML, which Quill drops because it cannot draw it.
fn is_html_block(text: &str) -> bool {
    if indent_of(text) >= 4 {
        return false;
    }
    let body = text.trim_start();
    if body.starts_with("<!--") {
        return true;
    }
    let after = body.strip_prefix('<').map(|rest| rest.strip_prefix('/').unwrap_or(rest));
    let Some(after) = after else { return false };
    let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if name.is_empty() {
        return false;
    }
    // What follows the name is what says this is a tag rather than an address in angle brackets.
    matches!(after[name.len()..].chars().next(), None | Some(' ') | Some('>') | Some('/'))
}

/// A quote's line with one `>` and one optional space taken off it.
fn quote_prefix(text: &str) -> Option<&str> {
    if indent_of(text) >= 4 {
        return None;
    }
    let body = text.trim_start();
    let rest = body.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// Read an image mark when it is the whole of a line, and nothing else.
///
/// An empty source is not a picture: it names no file, and a paragraph reserved for nothing would be
/// a gap in the page.
pub(crate) fn whole_line_image(body: &str) -> Option<(&str, &str)> {
    let rest = body.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = &rest[..close];
    let after = &rest[close + 2..];
    let end = after.rfind(')')?;
    if !after[end + 1..].trim().is_empty() {
        return None;
    }
    let source = after[..end].trim();
    if source.is_empty() {
        return None;
    }
    Some((source, alt))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(source: &str) -> Vec<Block> {
        parse(source).0
    }

    fn kinds(source: &str) -> Vec<String> {
        blocks(source)
            .iter()
            .map(|block| match &block.kind {
                Kind::Heading { level, .. } => format!("heading {level}"),
                Kind::Paragraph { .. } => "paragraph".to_owned(),
                Kind::Quote(_) => "quote".to_owned(),
                Kind::List(list) => {
                    format!("list {} {}", list.items.len(), if list.tight { "tight" } else { "loose" })
                }
                Kind::Code { .. } => "code".to_owned(),
                Kind::Diagram { .. } => "diagram".to_owned(),
                Kind::Table(_) => "table".to_owned(),
                Kind::Rule => "rule".to_owned(),
                Kind::FrontMatter(_) => "front matter".to_owned(),
                Kind::Image { .. } => "image".to_owned(),
                Kind::Footnote { .. } => "footnote".to_owned(),
            })
            .collect()
    }

    fn paragraph(block: &Block) -> &str {
        match &block.kind {
            Kind::Paragraph { content } => content,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn a_wrapped_paragraph_is_one_block() {
        let blocks = blocks("one line\nand the next\nand a third\n\nsecond paragraph");
        assert_eq!(kinds("one line\nand the next\n\nsecond"), ["paragraph", "paragraph"]);
        assert_eq!(paragraph(&blocks[0]), "one line\nand the next\nand a third");
    }

    #[test]
    fn a_setext_underline_makes_a_heading() {
        assert_eq!(kinds("Title\n====="), ["heading 1"]);
        assert_eq!(kinds("Title\n-----"), ["heading 2"]);
        // With nothing above it, the same line is a rule.
        assert_eq!(kinds("\n-----"), ["rule"]);
    }

    #[test]
    fn a_heading_loses_its_closing_hashes() {
        let blocks = blocks("## Title ##");
        match &blocks[0].kind {
            Kind::Heading { level, content } => {
                assert_eq!(*level, 2);
                assert_eq!(content, "Title");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_thematic_break_may_have_spaces_in_it() {
        assert_eq!(kinds("- - -"), ["rule"]);
        assert_eq!(kinds("***"), ["rule"]);
        assert_eq!(kinds("_ _ _ _"), ["rule"]);
    }

    #[test]
    fn a_quote_holds_blocks_rather_than_lines() {
        let blocks = blocks("> # Inside\n>\n> - a bullet\n> - another");
        match &blocks[0].kind {
            Kind::Quote(inner) => {
                assert!(matches!(inner[0].kind, Kind::Heading { level: 1, .. }));
                assert!(matches!(inner[1].kind, Kind::List(_)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_quote_inside_a_quote_is_two_deep() {
        let blocks = blocks("> outer\n> > inner");
        match &blocks[0].kind {
            Kind::Quote(inner) => {
                assert!(matches!(inner[1].kind, Kind::Quote(_)), "got {:?}", inner[1].kind)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_quote_continues_lazily() {
        let blocks = blocks("> one line\nand the next");
        match &blocks[0].kind {
            Kind::Quote(inner) => {
                assert_eq!(inner.len(), 1);
                assert_eq!(paragraph(&inner[0]), "one line\nand the next");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_list_item_can_hold_a_paragraph_and_a_fence() {
        let source = "- first\n\n  more of the first\n\n  ```\n  code\n  ```\n- second";
        let blocks = blocks(source);
        match &blocks[0].kind {
            Kind::List(list) => {
                assert_eq!(list.items.len(), 2);
                assert!(!list.tight, "a blank line between the items makes it loose");
                let kinds: Vec<_> = list.items[0]
                    .blocks
                    .iter()
                    .map(|block| matches!(block.kind, Kind::Code { .. }))
                    .collect();
                assert_eq!(kinds, [false, false, true]);
            }
            other => panic!("{other:?}"),
        }
    }

    /// A blank line between two lists is the gap between them, not a blank inside one. Getting this
    /// wrong made every list followed by a list of the other kind come out spread apart.
    #[test]
    fn a_list_followed_by_a_list_of_the_other_kind_is_still_tight() {
        assert_eq!(kinds("- a\n- b\n\n1. one\n2. two"), ["list 2 tight", "list 2 tight"]);
    }

    #[test]
    fn a_list_with_no_blank_lines_is_tight() {
        match &blocks("- a\n- b\n- c")[0].kind {
            Kind::List(list) => assert!(list.tight),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_nested_list_is_inside_its_item() {
        match &blocks("- outer\n  - inner\n  - also inner\n- second")[0].kind {
            Kind::List(list) => {
                assert_eq!(list.items.len(), 2);
                assert!(matches!(list.items[0].blocks[1].kind, Kind::List(_)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_ordered_list_keeps_the_number_it_starts_at() {
        match &blocks("3. third\n4. fourth")[0].kind {
            Kind::List(list) => {
                assert!(list.ordered);
                assert_eq!(list.start, 3);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_tick_box_is_read_off_the_item() {
        match &blocks("- [ ] to do\n- [x] done")[0].kind {
            Kind::List(list) => {
                assert_eq!(list.items[0].task, Some(false));
                assert_eq!(list.items[1].task, Some(true));
                assert_eq!(paragraph(&list.items[0].blocks[0]), "to do");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_year_at_the_start_of_a_line_is_not_a_list() {
        assert_eq!(kinds("It happened in\n2015. And then it stopped."), ["paragraph"]);
    }

    #[test]
    fn four_spaces_of_indent_is_a_code_block() {
        let blocks = blocks("text\n\n    indented code\n    more code\n\ntext again");
        assert_eq!(kinds("text\n\n    code\n\nmore"), ["paragraph", "code", "paragraph"]);
        match &blocks[1].kind {
            Kind::Code { lines, .. } => {
                assert_eq!(lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), [
                    "indented code",
                    "more code"
                ]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_fence_keeps_its_language_and_its_lines() {
        match &blocks("```rust\nfn main() {}\n```")[0].kind {
            Kind::Code { language, lines } => {
                assert_eq!(language, "rust");
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].text, "fn main() {}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_fence_nobody_closed_is_still_a_block() {
        assert_eq!(kinds("```\nstill going"), ["code"]);
        assert_eq!(kinds("```mermaid\ngraph TD"), ["diagram"]);
    }

    #[test]
    fn front_matter_is_read_only_at_the_front() {
        assert_eq!(kinds("---\ntitle: a\n---\n\nbody"), ["front matter", "paragraph"]);
        assert_eq!(kinds("body\n\n---\n\nmore"), ["paragraph", "rule", "paragraph"]);
    }

    #[test]
    fn a_link_definition_produces_no_block_and_is_remembered() {
        let (blocks, references) = parse("see [it][a]\n\n[a]: https://example.com");
        assert_eq!(blocks.len(), 1);
        assert_eq!(references.links, [("a".to_owned(), "https://example.com".to_owned())]);
    }

    #[test]
    fn a_footnote_definition_is_a_block_and_is_numbered() {
        let (blocks, references) = parse("a claim[^one]\n\n[^one]: because.");
        assert_eq!(references.footnotes, ["one"]);
        assert!(matches!(blocks[1].kind, Kind::Footnote { number: 1, .. }));
    }

    #[test]
    fn html_is_dropped_rather_than_shown() {
        assert_eq!(kinds("<div class=\"x\">\n  something\n</div>\n\ntext"), ["paragraph"]);
    }

    #[test]
    fn an_address_in_angle_brackets_is_not_html() {
        assert_eq!(kinds("<https://example.com>"), ["paragraph"]);
    }

    #[test]
    fn a_picture_on_its_own_line_is_a_block() {
        assert_eq!(kinds("![alt](picture.png)"), ["image"]);
        assert_eq!(kinds("words ![alt](picture.png) words"), ["paragraph"]);
    }

    #[test]
    fn every_block_knows_the_line_it_started_on() {
        let blocks = blocks("# One\n\nsome prose\n\n- a\n- b");
        assert_eq!(blocks.iter().map(|block| block.line).collect::<Vec<_>>(), [0, 2, 4]);
    }
}
