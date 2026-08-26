//! What the marks inside a line mean.
//!
//! The old preview walked a line with three booleans — bold, italic, struck — and flipped each one
//! when it met the mark for it. That is why `2 * 3 * 4` came out italic: a toggle has no opinion
//! about whether a `*` is the start of anything.
//!
//! This is CommonMark's own answer, which is a **delimiter stack**. A run of `*`, `_` or `~` is
//! measured, asked whether it may open and whether it may close, and matched against the runs still
//! open behind it. A run that can do neither is text, which is the whole of why `2 * 3 * 4` is now
//! left alone: neither `*` is followed by anything but a space, so neither can open.
//!
//! The scanner in front of the stack takes the things that win over emphasis, in this order:
//! backslash escapes, character references, code spans, autolinks, raw HTML, images, links and
//! footnotes. Order matters — a `*` inside a code span is a `*`.
//!
//! Everything comes out as a flat list of [`Span`]s. A span is text plus what it looks like, and
//! nothing else; `build` turns each into a `CharStyle` from the base style of whatever block it is
//! in, which is what lets one inline parser serve a heading, a table cell and a bullet.

use super::entity;

/// What a stretch of text is, beyond bold and italic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Ordinary text, in the colour of whatever block it is in.
    Text,
    /// Inline code: the monospaced family, the code colour, and a chip drawn behind it.
    Code,
    /// A link's label: the link colour, underlined. The address is not shown.
    Link,
    /// Said in the quiet colour: a picture's alt text, a footnote's number.
    Quiet,
    /// A hard line break. It carries no text and ends the line it is on.
    Break,
}

/// One stretch of a line, with everything about how it should look already decided.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub kind: Kind,
}

impl Span {
    /// Whether two spans look the same, and so can be folded into one.
    fn matches(&self, other: &Self) -> bool {
        self.bold == other.bold
            && self.italic == other.italic
            && self.strike == other.strike
            && self.kind == other.kind
            && self.kind != Kind::Break
    }
}

/// The definitions a document collected before its inlines were read.
///
/// A reference link may be used before it is defined, so the block parser gathers these in a pass of
/// its own and the inline parser looks up as it goes. A label is matched case-insensitively with its
/// inner whitespace collapsed, which is what the specification asks for.
#[derive(Debug, Clone, Default)]
pub(crate) struct References {
    /// Label to destination. The title is read and dropped: the preview shows a link's words and
    /// hides its address, so it has nowhere to put one.
    pub links: Vec<(String, String)>,
    /// The footnote labels, in the order they were defined, which is what numbers them.
    pub footnotes: Vec<String>,
}

impl References {
    /// The form a label is compared in.
    pub fn normalise(label: &str) -> String {
        label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    }

    fn link(&self, label: &str) -> Option<&str> {
        let wanted = Self::normalise(label);
        self.links.iter().find(|(name, _)| *name == wanted).map(|(_, to)| to.as_str())
    }

    fn footnote(&self, label: &str) -> Option<usize> {
        let wanted = Self::normalise(label);
        self.footnotes.iter().position(|name| *name == wanted).map(|at| at + 1)
    }
}

/// The tree the scanner builds before it is flattened into spans.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Text(String),
    Code(String),
    Quiet(String),
    Break,
    Link(Vec<Node>),
    Emph { bold: bool, italic: bool, strike: bool, children: Vec<Node> },
}

/// A delimiter run that is still waiting for something to close it.
struct Frame {
    /// The character it is made of. Three of them are not characters anybody types: they stand for
    /// the HTML tags that have a meaning here, so one stack answers both.
    mark: char,
    /// How much of the run is still unmatched.
    count: usize,
    /// How long the run was to start with, which is what the rule of three is asked about.
    original: usize,
    /// Whether the run could also have closed something, which the rule of three also asks.
    can_close: bool,
    nodes: Vec<Node>,
}

/// `<b>`, `<i>` and `<code>` are made to look like delimiter runs so that one stack closes
/// everything. No document contains these characters, so no run of them can be confused with one.
const HTML_BOLD: char = '\u{1}';
const HTML_ITALIC: char = '\u{2}';

/// Read the marks inside `text` and give back what it should look like.
///
/// `text` is one block's worth of source with its line breaks still in it, because whether a break
/// is hard depends on what came before it and that is a question about the source.
pub(crate) fn parse(text: &str, references: &References) -> Vec<Span> {
    let nodes = scan(text, references);
    let mut spans = Vec::new();
    flatten(&nodes, Span { text: String::new(), bold: false, italic: false, strike: false, kind: Kind::Text }, &mut spans);
    fold(spans)
}

/// Neighbouring spans that look the same become one, so a line of prose is one span rather than one
/// a word. Empty spans are dropped, except a break, which is empty by design.
fn fold(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.text.is_empty() && span.kind != Kind::Break {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.matches(&span) => last.text.push_str(&span.text),
            _ => out.push(span),
        }
    }
    out
}

/// Walk the nodes, carrying down what each one is inside.
fn flatten(nodes: &[Node], state: Span, out: &mut Vec<Span>) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Span { text: text.clone(), ..state.clone() }),
            Node::Code(text) => {
                out.push(Span { text: text.clone(), kind: Kind::Code, ..state.clone() })
            }
            Node::Quiet(text) => {
                out.push(Span { text: text.clone(), kind: Kind::Quiet, ..state.clone() })
            }
            Node::Break => out.push(Span { text: String::new(), kind: Kind::Break, ..state.clone() }),
            Node::Link(children) => {
                let inside = Span { kind: Kind::Link, ..state.clone() };
                flatten(children, inside, out);
            }
            Node::Emph { bold, italic, strike, children } => {
                let inside = Span {
                    bold: state.bold || *bold,
                    italic: state.italic || *italic,
                    strike: state.strike || *strike,
                    ..state.clone()
                };
                flatten(children, inside, out);
            }
        }
    }
}

/// The scanner: one pass over the characters, with a stack of open delimiter runs behind it.
fn scan(text: &str, references: &References) -> Vec<Node> {
    let mut stack: Vec<Frame> =
        vec![Frame { mark: '\0', count: 0, original: 0, can_close: false, nodes: Vec::new() }];
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut plain = String::new();

    while at < bytes.len() {
        let rest = &text[at..];
        let first = bytes[at];

        // A backslash escapes the punctuation after it, which is how a `*` is written without
        // meaning emphasis. In front of a line break it is a hard break, which is the other thing
        // CommonMark gives it.
        if first == b'\\' {
            match rest[1..].chars().next() {
                Some('\n') => {
                    push_text(&mut stack, &mut plain);
                    top(&mut stack).nodes.push(Node::Break);
                    at += 2;
                }
                Some(next) if is_punctuation(next) => {
                    plain.push(next);
                    at += 1 + next.len_utf8();
                }
                _ => {
                    plain.push('\\');
                    at += 1;
                }
            }
            continue;
        }

        if first == b'&' {
            if let Some((character, length)) = entity::read(rest) {
                plain.push_str(&character);
                at += length;
                continue;
            }
        }

        // A line break inside a block. Two trailing spaces make it a hard one; otherwise it is a
        // space, which is what makes hand-wrapped prose come out as one paragraph.
        if first == b'\n' {
            let hard = text[..at].ends_with("  ");
            while plain.ends_with(' ') {
                plain.pop();
            }
            push_text(&mut stack, &mut plain);
            if hard {
                top(&mut stack).nodes.push(Node::Break);
            } else {
                plain.push(' ');
            }
            at += 1;
            continue;
        }

        if first == b'`' {
            if let Some((code, length)) = read_code_span(rest) {
                push_text(&mut stack, &mut plain);
                top(&mut stack).nodes.push(Node::Code(code));
                at += length;
                continue;
            }
        }

        if first == b'<' {
            // The text so far is flushed *before* the tag is read, because reading an opening or a
            // closing tag moves the stack, and text still waiting in the buffer would land in the
            // wrong frame. A tag that turns out to be nothing has cost one extra text node, which
            // `fold` puts back together.
            push_text(&mut stack, &mut plain);
            if let Some((node, length)) = read_angle(rest, &mut stack) {
                if let Some(node) = node {
                    top(&mut stack).nodes.push(node);
                }
                at += length;
                continue;
            }
        }

        // A picture inside a line of prose is its alt text, in the quiet colour: a picture in the
        // middle of a paragraph needs inline layout the engine does not have. Before the link
        // below, so the `!` is not left behind as a stray character.
        if rest.starts_with("![") {
            if let Some(read) = read_link(rest, references, true) {
                push_text(&mut stack, &mut plain);
                if let Some(node) = read.node {
                    top(&mut stack).nodes.push(node);
                }
                at += read.length;
                continue;
            }
        }

        if first == b'[' {
            if let Some(read) = read_link(rest, references, false) {
                push_text(&mut stack, &mut plain);
                if let Some(node) = read.node {
                    top(&mut stack).nodes.push(node);
                }
                at += read.length;
                continue;
            }
        }

        if matches!(first, b'*' | b'_' | b'~') {
            let run = delimiter_run(text, at);
            push_text(&mut stack, &mut plain);
            emphasis(&mut stack, run);
            at += run.count;
            continue;
        }

        // A bare address, which GFM turns into a link and CommonMark does not. Only at the start of
        // a word, so `see.www.example` is not one.
        if (first == b'h' || first == b'w') && at_word_start(text, at) {
            if let Some(length) = read_bare_link(rest) {
                push_text(&mut stack, &mut plain);
                let address = &rest[..length];
                top(&mut stack).nodes.push(Node::Link(vec![Node::Text(address.to_owned())]));
                at += length;
                continue;
            }
        }

        let character = rest.chars().next().unwrap_or('\u{FFFD}');
        plain.push(character);
        at += character.len_utf8();
    }

    push_text(&mut stack, &mut plain);
    while stack.len() > 1 {
        unwind(&mut stack);
    }
    stack.pop().map(|frame| frame.nodes).unwrap_or_default()
}

/// The frame everything is being added to.
fn top(stack: &mut [Frame]) -> &mut Frame {
    stack.last_mut().expect("the root frame is never popped")
}

/// Move the plain text collected so far into the open frame.
fn push_text(stack: &mut [Frame], plain: &mut String) {
    if plain.is_empty() {
        return;
    }
    let text = std::mem::take(plain);
    top(stack).nodes.push(Node::Text(text));
}

/// Close the top frame as literal text: whatever opened it was never matched, so it was a
/// character rather than a mark.
fn unwind(stack: &mut Vec<Frame>) {
    let frame = stack.pop().expect("only called with something above the root");
    let literal = match frame.mark {
        HTML_BOLD | HTML_ITALIC => String::new(),
        mark => mark.to_string().repeat(frame.count),
    };
    let parent = top(stack);
    if !literal.is_empty() {
        parent.nodes.push(Node::Text(literal));
    }
    parent.nodes.extend(frame.nodes);
}

/// A run of the same delimiter character, and what it is allowed to do.
#[derive(Debug, Clone, Copy)]
struct Run {
    mark: char,
    count: usize,
    can_open: bool,
    can_close: bool,
}

/// Measure the run of delimiters starting at `at` and apply CommonMark's flanking rules.
///
/// A run is *left-flanking* when it is not followed by whitespace and either is not followed by
/// punctuation or is preceded by whitespace or punctuation; *right-flanking* is the mirror of that.
/// The extra clause for `_` is what makes `snake_case_word` one word — the general rule, in place of
/// the special case the old parser carried for underscores alone.
fn delimiter_run(text: &str, at: usize) -> Run {
    let mark = text[at..].chars().next().unwrap_or('*');
    let count = text[at..].chars().take_while(|c| *c == mark).count();
    let before = text[..at].chars().last();
    let after = text[at + count..].chars().next();
    let before_space = before.is_none_or(char::is_whitespace);
    let after_space = after.is_none_or(char::is_whitespace);
    let before_punct = before.is_some_and(is_punctuation);
    let after_punct = after.is_some_and(is_punctuation);

    let left = !after_space && (!after_punct || before_space || before_punct);
    let right = !before_space && (!before_punct || after_space || after_punct);

    let (can_open, can_close) = match mark {
        '_' => (left && (!right || before_punct), right && (!left || after_punct)),
        // GFM's strikethrough is one or two tildes and no more. A longer run is text, which is what
        // stops a row of tildes used as a rule from striking the rest of the document through.
        '~' if count > 2 => (false, false),
        _ => (left, right),
    };
    Run { mark, count, can_open, can_close }
}

/// True for a character CommonMark counts as punctuation, which is anything that is neither a
/// letter, a number nor a space.
fn is_punctuation(character: char) -> bool {
    !character.is_alphanumeric() && !character.is_whitespace()
}

/// Match a delimiter run against the runs still open, then open what is left of it.
fn emphasis(stack: &mut Vec<Frame>, run: Run) {
    let mut remaining = run.count;
    if run.can_close {
        while remaining > 0 {
            let Some(index) = opener(stack, run.mark, remaining, run.can_open) else {
                break;
            };
            while stack.len() > index + 1 {
                unwind(stack);
            }
            let take = if top(stack).count >= 2 && remaining >= 2 { 2 } else { 1 };
            let frame = top(stack);
            let children = std::mem::take(&mut frame.nodes);
            let node = match run.mark {
                '~' => Node::Emph { bold: false, italic: false, strike: true, children },
                _ => Node::Emph { bold: take == 2, italic: take == 1, strike: false, children },
            };
            frame.count -= take;
            if frame.count == 0 {
                stack.pop();
                top(stack).nodes.push(node);
            } else {
                frame.nodes = vec![node];
            }
            remaining -= take;
        }
    }
    if remaining == 0 {
        return;
    }
    if run.can_open {
        stack.push(Frame {
            mark: run.mark,
            count: remaining,
            original: run.count,
            can_close: run.can_close,
            nodes: Vec::new(),
        });
    } else {
        top(stack).nodes.push(Node::Text(run.mark.to_string().repeat(remaining)));
    }
}

/// The open run this closer should pair with, if any.
///
/// The rule of three is CommonMark's answer to `*foo**bar**baz*`: when either side could both open
/// and close, the two lengths added together must not be a multiple of three unless both of them
/// are. Without it that line comes out with the emphasis nested the wrong way round.
fn opener(stack: &[Frame], mark: char, closer: usize, closer_opens: bool) -> Option<usize> {
    for index in (1..stack.len()).rev() {
        let frame = &stack[index];
        if frame.mark != mark {
            continue;
        }
        let both_ways = frame.can_close || closer_opens;
        let sum = frame.original + closer;
        if both_ways && sum % 3 == 0 && !(frame.original % 3 == 0 && closer % 3 == 0) {
            continue;
        }
        return Some(index);
    }
    None
}

/// Read a code span, which is a run of backticks closed by a run of exactly the same length.
///
/// One leading and one trailing space are stripped when what is left is not all spaces, which is
/// what makes `` `` `code` `` `` show a backtick rather than a space either side of one.
fn read_code_span(rest: &str) -> Option<(String, usize)> {
    let opening = rest.chars().take_while(|c| *c == '`').count();
    let after = &rest[opening..];
    let mut at = 0;
    while at < after.len() {
        let start = at + after[at..].find('`')?;
        let run = after[start..].chars().take_while(|c| *c == '`').count();
        if run == opening {
            let mut inner = &after[..start];
            if inner.len() >= 2
                && inner.starts_with(' ')
                && inner.ends_with(' ')
                && !inner.trim().is_empty()
            {
                inner = &inner[1..inner.len() - 1];
            }
            // Line breaks inside a code span are spaces, since the span is one run of text.
            return Some((inner.replace('\n', " "), opening + start + run));
        }
        at = start + run;
    }
    None
}

/// True when the byte at `at` starts a word, so a bare address is not found in the middle of one.
fn at_word_start(text: &str, at: usize) -> bool {
    match text[..at].chars().last() {
        None => true,
        Some(previous) => !previous.is_alphanumeric() && previous != '.' && previous != '/',
    }
}

/// Read a bare address, which GFM turns into a link.
///
/// Trailing punctuation is left out, because a sentence ending in an address puts a full stop after
/// it and nobody means the full stop to be part of the link. A closing bracket is kept only when
/// the address has an opening one to match it.
fn read_bare_link(rest: &str) -> Option<usize> {
    let lower = rest.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") && !lower.starts_with("www.")
    {
        return None;
    }
    let mut end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if lower.starts_with("www.") && !rest[..end].contains('.') {
        return None;
    }
    while end > 0 {
        let last = rest[..end].chars().last().unwrap_or(' ');
        let unbalanced = last == ')'
            && rest[..end].matches('(').count() < rest[..end].matches(')').count();
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '_') || unbalanced {
            end -= last.len_utf8();
            continue;
        }
        break;
    }
    // A scheme and nothing after it is not an address.
    let shortest = if lower.starts_with("www.") { 5 } else { 9 };
    (end >= shortest).then_some(end)
}

/// What reading something in angle brackets came to: a node to add, or nothing when it was a tag
/// that Quill cannot draw and drops.
fn read_angle(rest: &str, stack: &mut Vec<Frame>) -> Option<(Option<Node>, usize)> {
    let end = rest.find('>')?;
    let inner = &rest[1..end];
    let length = end + 1;
    if inner.is_empty() || inner.contains('<') || inner.contains(char::is_whitespace) {
        // A tag with attributes in it still ends at the first `>`, so the whitespace test alone
        // would refuse `<a href="x">`. Fall through to the tag reader below.
        if let Some(name) = tag_name(inner) {
            return Some((html_tag(&name, inner, stack), length));
        }
        return None;
    }
    // An autolink: a scheme and a colon, or an email address.
    if inner.contains(':') && !inner.starts_with(':') {
        return Some((Some(Node::Link(vec![Node::Text(inner.to_owned())])), length));
    }
    if inner.contains('@') && inner.contains('.') {
        return Some((Some(Node::Link(vec![Node::Text(inner.to_owned())])), length));
    }
    let name = tag_name(inner)?;
    Some((html_tag(&name, inner, stack), length))
}

/// The tag name inside angle brackets, lowercased, or nothing when this is not a tag at all.
fn tag_name(inner: &str) -> Option<String> {
    let body = inner.strip_prefix('/').unwrap_or(inner);
    let name: String = body.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if name.is_empty() {
        return None;
    }
    Some(name.to_lowercase())
}

/// What one HTML tag means here.
///
/// Quill has no HTML engine and will not grow one, so a tag is not shown. Four of them have an
/// obvious equivalent in styled text and are given it; the rest are dropped, because a reader who
/// sees `<sub>` in the middle of a sentence is worse off than one who sees neither.
fn html_tag(name: &str, inner: &str, stack: &mut Vec<Frame>) -> Option<Node> {
    let closing = inner.starts_with('/');
    match (name, closing) {
        ("br", _) => Some(Node::Break),
        ("b" | "strong", false) => {
            open_html(stack, HTML_BOLD);
            None
        }
        ("i" | "em", false) => {
            open_html(stack, HTML_ITALIC);
            None
        }
        ("b" | "strong", true) => close_html(stack, HTML_BOLD, true),
        ("i" | "em", true) => close_html(stack, HTML_ITALIC, false),
        _ => None,
    }
}

fn open_html(stack: &mut Vec<Frame>, mark: char) {
    stack.push(Frame { mark, count: 1, original: 1, can_close: false, nodes: Vec::new() });
}

/// Close an HTML tag that was opened. A closing tag with nothing open is dropped, which is what
/// happens to every other tag.
fn close_html(stack: &mut Vec<Frame>, mark: char, bold: bool) -> Option<Node> {
    let index = (1..stack.len()).rev().find(|index| stack[*index].mark == mark)?;
    while stack.len() > index + 1 {
        unwind(stack);
    }
    let frame = stack.pop()?;
    Some(Node::Emph { bold, italic: !bold, strike: false, children: frame.nodes })
}

/// What reading a link or a picture came to.
struct ReadLink {
    node: Option<Node>,
    length: usize,
}

/// Read `[label](destination)`, `[label][reference]`, `[reference]` or `[^footnote]`.
///
/// A reference nothing defines is left as the text it was written as, which is what CommonMark says
/// and is also what a reader wants: a broken link should look broken.
fn read_link(rest: &str, references: &References, picture: bool) -> Option<ReadLink> {
    let open = if picture { 2 } else { 1 };
    let close = matching_bracket(&rest[open - 1..])? + open - 1;
    let label = &rest[open..close];
    let after = &rest[close + 1..];

    // A footnote, which is a label starting with a caret and nothing after it.
    if !picture {
        if let Some(name) = label.strip_prefix('^') {
            if let Some(number) = references.footnote(name) {
                return Some(ReadLink {
                    node: Some(Node::Quiet(format!("[{number}]"))),
                    length: close + 1,
                });
            }
            return None;
        }
    }

    // An inline destination.
    if after.starts_with('(') {
        if let Some(end) = destination_end(after) {
            return Some(ReadLink {
                node: Some(label_node(label, picture, references)),
                length: close + 1 + end + 1,
            });
        }
    }

    // A reference: either named, or the label standing for itself.
    if let Some(second) = after.strip_prefix('[') {
        if let Some(end) = second.find(']') {
            let name = if second[..end].trim().is_empty() { label } else { &second[..end] };
            if references.link(name).is_some() {
                return Some(ReadLink {
                    node: Some(label_node(label, picture, references)),
                    length: close + 1 + end + 2,
                });
            }
            return None;
        }
    }
    if references.link(label).is_some() {
        return Some(ReadLink { node: Some(label_node(label, picture, references)), length: close + 1 });
    }
    None
}

/// A link's label goes through the whole inline pass, because it can hold marks of its own. A
/// picture's alt text does not: it stands in for a picture and is shown plainly.
fn label_node(label: &str, picture: bool, references: &References) -> Node {
    if picture {
        return Node::Quiet(label.to_owned());
    }
    Node::Link(scan(label, references))
}

/// Where the `]` matching the `[` at the start of `rest` is, counting nesting and skipping escapes.
fn matching_bracket(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            b'`' => {
                if let Some((_, length)) = read_code_span(&rest[at..]) {
                    at += length;
                    continue;
                }
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// Where the `)` closing an inline destination is. Nesting is counted so that a link to an address
/// with brackets in it is read whole, and an address in angle brackets may hold anything.
fn destination_end(after: &str) -> Option<usize> {
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut at = 0;
    let mut angled = false;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            b'<' if depth == 1 => angled = true,
            b'>' if angled => angled = false,
            b'(' if !angled => depth += 1,
            b')' if !angled => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            b'\n' if !angled => return None,
            _ => {}
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<Span> {
        parse(text, &References::default())
    }

    fn plain(text: &str) -> String {
        spans(text).into_iter().map(|span| span.text).collect()
    }

    /// The span covering the first occurrence of `needle`.
    fn at(text: &str, needle: &str) -> Span {
        spans(text)
            .into_iter()
            .find(|span| span.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is in none of {:?}", spans(text)))
    }

    #[test]
    fn emphasis_is_read_and_its_marks_are_not_shown() {
        assert_eq!(plain("**bold** and *italic*"), "bold and italic");
        assert!(at("**bold**", "bold").bold);
        assert!(at("*italic*", "italic").italic);
        assert!(at("~~gone~~", "gone").strike);
    }

    /// The fault the delimiter stack was written for. A toggle made this italic.
    #[test]
    fn arithmetic_is_not_emphasis() {
        assert_eq!(plain("2 * 3 * 4"), "2 * 3 * 4");
        assert!(!at("2 * 3 * 4", "2").italic);
    }

    #[test]
    fn an_underscore_inside_a_word_is_part_of_the_word() {
        assert_eq!(plain("a snake_case_word here"), "a snake_case_word here");
        assert_eq!(plain("_this_ is emphasis"), "this is emphasis");
    }

    #[test]
    fn three_marks_are_bold_and_italic_at_once() {
        let span = at("***both***", "both");
        assert!(span.bold && span.italic);
    }

    #[test]
    fn a_mark_nobody_closed_is_a_character() {
        assert_eq!(plain("a * b"), "a * b");
        assert_eq!(plain("**unclosed"), "**unclosed");
    }

    #[test]
    fn emphasis_nests() {
        assert_eq!(plain("*outer **inner** outer*"), "outer inner outer");
        let inner = at("*outer **inner** outer*", "inner");
        assert!(inner.bold && inner.italic);
    }

    #[test]
    fn a_code_span_holds_marks_rather_than_reading_them() {
        assert_eq!(plain("say `**not bold**` here"), "say **not bold** here");
        assert_eq!(at("say `**not bold**`", "not bold").kind, Kind::Code);
    }

    #[test]
    fn a_longer_run_of_backticks_lets_a_backtick_be_shown() {
        assert_eq!(plain("`` a ` b ``"), "a ` b");
    }

    #[test]
    fn a_backslash_escapes_the_mark_after_it() {
        assert_eq!(plain("\\*not italic\\*"), "*not italic*");
        assert!(!at("\\*not italic\\*", "not italic").italic);
    }

    #[test]
    fn a_character_reference_becomes_its_character() {
        assert_eq!(plain("A &amp; B &mdash; C"), "A & B \u{2014} C");
    }

    #[test]
    fn a_link_shows_its_words_and_hides_its_address() {
        assert_eq!(plain("see [the design](https://example.com/x) now"), "see the design now");
        let label = at("see [the design](https://example.com/x)", "the design");
        assert_eq!(label.kind, Kind::Link);
    }

    #[test]
    fn a_link_title_is_not_part_of_the_address_or_the_words() {
        assert_eq!(plain("[a](http://x \"the title\")"), "a");
    }

    #[test]
    fn a_reference_link_is_read_when_something_defines_it() {
        let references = References {
            links: vec![("ref".to_owned(), "https://example.com".to_owned())],
            footnotes: Vec::new(),
        };
        let spans = parse("see [the words][ref] now", &references);
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "see the words now");
        assert!(spans.iter().any(|span| span.kind == Kind::Link));
    }

    #[test]
    fn a_reference_nothing_defines_is_left_as_it_was_written() {
        assert_eq!(plain("see [the words][missing] now"), "see [the words][missing] now");
    }

    #[test]
    fn a_bare_address_becomes_a_link_without_its_full_stop() {
        let spans = spans("go to https://example.com/a. Then stop.");
        let link = spans.iter().find(|span| span.kind == Kind::Link).expect("a link");
        assert_eq!(link.text, "https://example.com/a");
    }

    #[test]
    fn an_autolink_in_angle_brackets_is_a_link() {
        assert_eq!(plain("<https://example.com>"), "https://example.com");
        assert_eq!(at("<https://example.com>", "example").kind, Kind::Link);
        assert_eq!(at("<a@b.com>", "a@b.com").kind, Kind::Link);
    }

    #[test]
    fn a_tag_quill_cannot_draw_is_not_shown() {
        assert_eq!(plain("a <span class=\"x\">word</span> here"), "a word here");
    }

    #[test]
    fn the_four_tags_with_a_meaning_have_it() {
        assert!(at("<b>bold</b>", "bold").bold);
        assert!(at("<em>slanted</em>", "slanted").italic);
        assert!(spans("one<br>two").iter().any(|span| span.kind == Kind::Break));
    }

    #[test]
    fn a_wrapped_paragraph_is_one_paragraph() {
        assert_eq!(plain("one line\nand the next"), "one line and the next");
    }

    #[test]
    fn two_trailing_spaces_are_a_hard_break() {
        let spans = spans("one line  \nand the next");
        assert!(spans.iter().any(|span| span.kind == Kind::Break));
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "one lineand the next");
    }

    #[test]
    fn a_picture_inside_a_line_is_its_alt_text() {
        let spans = spans("before ![the alt](x.png) after");
        let quiet = spans.iter().find(|span| span.kind == Kind::Quiet).expect("the alt text");
        assert_eq!(quiet.text, "the alt");
    }

    #[test]
    fn a_footnote_marker_is_numbered_when_something_defines_it() {
        let references =
            References { links: Vec::new(), footnotes: vec!["one".to_owned(), "two".to_owned()] };
        let spans = parse("a claim[^two] here", &references);
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "a claim[2] here");
    }

    #[test]
    fn neighbouring_spans_that_look_the_same_are_one_span() {
        assert_eq!(spans("just some ordinary words").len(), 1);
    }
}
