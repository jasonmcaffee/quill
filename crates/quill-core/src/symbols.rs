//! Where a name is defined, and everywhere it is used.
//!
//! Go to definition, find all references and rename are one mechanism wearing three faces:
//! something that, given a name in a file, can say where that name is defined and where it is used.
//! This module is that mechanism, and like [`crate::syntax`] beside it, it draws nothing, touches
//! no disk and its tests run with no window.
//!
//! ## What it knows, and what it does not
//!
//! It is **syntactic**. It reads the token stream the tokeniser already produces and applies the
//! rules a language's plugin manifest gives it; it has no scopes, no types and no idea what an
//! import means. `tasks/task-1675-code-editing-tdd.md` §2 weighs the two mechanisms that would know
//! those things — a language server client and tree-sitter with stack graphs — and records why
//! neither is what Quill is. This is the tier Sublime Text's goto-definition and GitHub's shipped
//! code navigation are: instant, predictable, and honest about ambiguity.
//!
//! **Honest** is the whole of the design. Where the mechanism cannot tell two same-named things
//! apart it says so by offering both, never by silently choosing one; a definition found by a
//! shape heuristic rather than by a keyword is marked [`Confidence::Likely`] and stays marked all
//! the way to the screen; and an occurrence inside a comment or a string carries the [`Role`] that
//! says which, because a rename that quietly rewrote a word inside a string would be a corruption
//! nobody asked for.
//!
//! ## The three questions
//!
//! - [`file_definitions`] — every definition in one file, from one pass over its tokens.
//! - [`identifier_at`] — the word under a point, or nothing when the point is not on a word.
//! - [`occurrences`] — every whole-word occurrence of a name, each labelled with its role.
//!
//! [`rank`] puts a list of candidate definitions in the order they should be offered, and
//! [`replacements`] is the arithmetic a rename is applied by. All of them are pure functions over
//! text and a [`Grammar`]; nothing here knows about files, threads or panes.

use std::ops::Range;

use crate::syntax::{self, Grammar, Token};

/// What kind of thing a definition names, for ranking and for the label a modal shows.
///
/// A language says which keyword makes which kind through `language.definers`, so the list is what
/// the five kinds of thing a syntactic reading can tell apart, rather than a taxonomy of any one
/// language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Type,
    Constant,
    Variable,
    Module,
}

impl SymbolKind {
    /// The name a manifest uses on the right of a `keyword=kind` pair, and the word a modal shows.
    pub fn name(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Type => "type",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Module => "module",
        }
    }

    /// The kind of this name, or nothing when a manifest asked for one that does not exist.
    ///
    /// Read rather than assumed, for the reason `plugin.kind` is: a manifest naming something this
    /// version does not have should say so rather than load as half a language.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "function" => Some(SymbolKind::Function),
            "type" => Some(SymbolKind::Type),
            "constant" => Some(SymbolKind::Constant),
            "variable" => Some(SymbolKind::Variable),
            "module" => Some(SymbolKind::Module),
            _ => None,
        }
    }

    /// Every kind, for a message that has to list them.
    pub const ALL: [SymbolKind; 5] = [
        SymbolKind::Function,
        SymbolKind::Type,
        SymbolKind::Constant,
        SymbolKind::Variable,
        SymbolKind::Module,
    ];

    /// True when renaming this kind of thing defaults to the whole project rather than to one file.
    ///
    /// A variable is the one that does not: a local or a parameter of the same name in another file
    /// is very rarely the same thing, and defaulting to the project would tick a hundred rows a
    /// person then has to untick. Everything else is named once and used everywhere, which is the
    /// scoping instinct behind IntelliJ disabling text occurrences for locals.
    pub fn renames_the_project(self) -> bool {
        !matches!(self, SymbolKind::Variable)
    }
}

/// How sure the mechanism is that this really is a definition.
///
/// `Sure` came from a definer keyword the language named. `Likely` came from a shape heuristic —
/// today that is only [`Grammar::brace_definitions`], the class method with no keyword in front of
/// it — and it is carried all the way to the screen rather than being rounded up to certainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    Sure,
    Likely,
}

/// One definition: where the name itself is, what it names, and how it was found.
///
/// The range is the **identifier's**, not the whole declaration's, because everything that uses it
/// wants the name: a jump selects it, a rename replaces it, and a modal shows the line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name_range: Range<usize>,
    pub kind: SymbolKind,
    pub confidence: Confidence,
}

/// Where an occurrence of a name sits in the file's own reading of itself.
///
/// Every reference list and every rename treats the three differently: code is the answer, and a
/// word inside a comment or a string is a textual match that is shown second-class and left
/// unticked, because the mechanism cannot tell a doc comment mentioning `draw` from prose that
/// happens to use the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Code,
    Comment,
    String,
}

impl Role {
    /// What a row is suffixed with, and what the command line prints. Code carries no suffix,
    /// because it is the ordinary answer.
    pub fn suffix(self) -> &'static str {
        match self {
            Role::Code => "",
            Role::Comment => "comment",
            Role::String => "string",
        }
    }

    /// The name the command line prints and a test compares against.
    pub fn name(self) -> &'static str {
        match self {
            Role::Code => "code",
            Role::Comment => "comment",
            Role::String => "string",
        }
    }
}

/// One occurrence: where it is, and what it was found inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub range: Range<usize>,
    pub role: Role,
}

/// One file's reading of itself: what it defines, where its words are, and which stretches of it
/// are comments and strings.
///
/// **Read once, asked many times.** The three questions below can each be asked as a free function
/// over a `&str`, and for a one-off — the command line, a test, a closed file being indexed — that
/// is what they are. But the hover query runs while the pointer moves with the modifier held, and
/// `task-1666`'s rule is that nothing which runs once a frame may read the whole file: one scan of a
/// 170 kilobyte source is 1.4 ms, which is a tenth of a frame spent on a question whose answer has
/// not changed. So the window builds one of these per open tab, keyed on `Document::text_revision()`
/// — the same key `colour_the_file` is keyed on — and every hover after the first is a binary
/// search over `words`.
///
/// It is deliberately not a copy of the token stream. Two sorted lists of ranges answer everything
/// asked of it, and a list of every token would be several times the size for nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSymbols {
    definitions: Vec<Definition>,
    /// The identifier-shaped tokens, in order.
    words: Vec<Range<usize>>,
    /// The comment and string spans, in order, so the role of a match is a binary search.
    quiet: Vec<(Range<usize>, Role)>,
}

impl FileSymbols {
    /// Read a file. One linear pass over the tokeniser's output, and the only one.
    ///
    /// The definition rule is the same shape as the tokeniser's own: **a definer keyword followed by
    /// a word makes that word a definition** of the kind the grammar gave the keyword. `fn draw`
    /// defines `draw` as a function; `struct Layout` defines `Layout` as a type; `const LIMIT`
    /// defines `LIMIT` as a constant.
    ///
    /// Three details the rule needs and nothing else does.
    ///
    /// A keyword the language did **not** name as a definer is stepped over rather than ending the
    /// run, because `let mut count` and `pub const LIMIT` are ordinary and the word after the second
    /// keyword is still the name.
    ///
    /// **Only whitespace on one line may separate the two.** Not "anything the tokeniser did not
    /// report": a grammar names the characters it draws as operators and a bracket is not usually
    /// among them, so a rule that waited for an operator to end the run would read
    /// `let (a, b) = split()` as defining `a`. What has to hold is that the keyword and the name are
    /// next to each other, which is what a declaration looks like in every language that has one.
    ///
    /// And a **builtin is never a definition**: a name the language itself provides cannot be
    /// declared by a file that uses it, which is what keeps `let Some(value)` and `let Ok(reply)`
    /// from defining `Some` and `Ok` in every Rust file there is.
    ///
    /// Nothing clever is skipped: a token the tokeniser classified `Comment` or `String` can never
    /// hold a definition, because the tokeniser already said what it is.
    pub fn read(text: &str, grammar: &Grammar) -> Self {
        let mut read = FileSymbols::default();
        let defines = grammar.defines_symbols();
        // The kind a definer keyword is waiting to give to the next word, and where that keyword
        // ended, so a line break between the two can end the run.
        let mut pending: Option<(SymbolKind, usize)> = None;
        syntax::scan(text, grammar, |range, token| {
            // The keyword and the name have to be next to each other. See the note above.
            let adjacent = |after: usize| {
                after <= range.start
                    && text[after..range.start].chars().all(|letter| letter == ' ' || letter == '\t')
            };
            match token {
                Token::Comment => read.quiet.push((range, Role::Comment)),
                Token::String => read.quiet.push((range, Role::String)),
                // A definer replaces whatever was pending, so `const fn new` defines a function. A
                // keyword the language did not name is stepped over: `let mut count` is a `count`.
                Token::Keyword => {
                    if let Some(kind) = grammar.definer(&text[range.clone()]) {
                        pending = Some((kind, range.end));
                    } else if let Some((kind, after)) = pending {
                        pending = adjacent(after).then_some((kind, range.end));
                    }
                }
                Token::Text | Token::Function | Token::Type | Token::Builtin => {
                    read.words.push(range.clone());
                    if !defines {
                        return;
                    }
                    // A builtin is a name the language provides, so this file is not declaring it.
                    if token != Token::Builtin {
                        if let Some((kind, after)) = pending.take() {
                            if adjacent(after) {
                                read.definitions.push(Definition {
                                    name_range: range,
                                    kind,
                                    confidence: Confidence::Sure,
                                });
                                return;
                            }
                        }
                        if grammar.brace_definitions
                            && token == Token::Function
                            && is_brace_definition(text, &range)
                        {
                            read.definitions.push(Definition {
                                name_range: range,
                                kind: SymbolKind::Function,
                                confidence: Confidence::Likely,
                            });
                        }
                    } else {
                        pending = None;
                    }
                }
                // Anything else ends the run: an operator and a number.
                _ => pending = None,
            }
        });
        read
    }

    /// What this file defines, in the order the definitions appear in it.
    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    /// The identifier under `offset`, or nothing when the point is not a question about a symbol.
    ///
    /// Nothing is the answer for a keyword, an operator, a number, and anywhere inside a comment or
    /// a string: a click on `return` is not a question about a symbol, and neither is one in the
    /// middle of a sentence in a doc comment.
    ///
    /// A point sitting exactly at the **end** of a word counts as being on it, because that is where
    /// a caret lands after a double click and is where `Rename Symbol` is asked from. Not inside a
    /// comment, though: a word ending exactly where a comment begins is the comment's.
    pub fn identifier_at(&self, offset: usize) -> Option<Range<usize>> {
        let after = match self.words.binary_search_by(|word| compare(word, offset)) {
            Ok(index) => return Some(self.words[index].clone()),
            Err(after) => after,
        };
        if self.role_at(offset) != Role::Code {
            return None;
        }
        // The word that ends exactly here is the one before the point, if any: the list is sorted
        // and the ranges do not overlap, so there is nowhere else it could be.
        self.words[..after].last().filter(|word| word.end == offset).cloned()
    }

    /// Every whole-word occurrence of `name`, each labelled with the role of the token it fell
    /// inside.
    ///
    /// Whole word means bounded by characters that are not word characters **for this grammar**,
    /// which is what stops `count` matching inside `counter` and `x` inside `x2`, and what lets a
    /// hyphen bound a word in Rust while being inside one in CSS.
    ///
    /// The text is passed back in rather than kept, because a copy of every open file's text held
    /// beside its own document would be the same bytes twice.
    pub fn occurrences(&self, text: &str, name: &str, grammar: &Grammar) -> Vec<Occurrence> {
        let mut found = Vec::new();
        if name.is_empty() || text.is_empty() {
            return found;
        }
        let mut at = 0;
        while let Some(index) = text[at..].find(name) {
            let start = at + index;
            let end = start + name.len();
            if is_whole_word(text, start, end, grammar) {
                found.push(Occurrence { range: start..end, role: self.role_at(start) });
            }
            at = start + name.chars().next().map_or(1, char::len_utf8);
            if at >= text.len() {
                break;
            }
        }
        found
    }

    /// Which comment or string a position falls in, if it falls in one. The spans are in order and
    /// do not overlap, so this is a binary search.
    pub fn role_at(&self, at: usize) -> Role {
        match self.quiet.binary_search_by(|(range, _)| compare(range, at)) {
            Ok(index) => self.quiet[index].1,
            Err(_) => Role::Code,
        }
    }

    /// How many words there are, which is what the measuring instrument reports.
    pub fn words(&self) -> usize {
        self.words.len()
    }

    /// Every distinct spelling of an identifier in this file, sorted, each one once.
    ///
    /// What completion offers for the locals, the parameters, the field names and everything else
    /// the definers cannot see — and the only thing it can offer in a language that deliberately
    /// named no definers, which is what makes it work in a stylesheet.
    ///
    /// Derived from [`Self::words`] rather than collected in the read, because the read runs
    /// whenever the text changes and `task-1666`'s rule is that nothing running that often may
    /// allocate more than it already does: a file with eleven thousand words has a few hundred
    /// spellings, and the window builds this list once a text revision beside the definitions it
    /// already builds there.
    ///
    /// Sorted so that a caller can say two files hold the same words by comparing the lists, and so
    /// that the order a stem is scored in is the file's spelling rather than its layout.
    pub fn distinct_words(&self, text: &str) -> Vec<String> {
        let mut spellings: Vec<&str> = self
            .words
            .iter()
            .filter_map(|word| text.get(word.clone()))
            .collect();
        spellings.sort_unstable();
        spellings.dedup();
        spellings.into_iter().map(str::to_owned).collect()
    }
}

/// Where `at` sits relative to a range, for a binary search over ranges that do not overlap.
fn compare(range: &Range<usize>, at: usize) -> std::cmp::Ordering {
    if range.end <= at {
        std::cmp::Ordering::Less
    } else if range.start > at {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// Every definition in one file, in the order they appear in it.
///
/// The one-off form of [`FileSymbols::read`], for a file being indexed or a command line answering
/// one question. Anything asking more than once — the window, with a tab open — reads once and keeps
/// it.
pub fn file_definitions(text: &str, grammar: &Grammar) -> Vec<Definition> {
    if !grammar.defines_symbols() {
        return Vec::new();
    }
    FileSymbols::read(text, grammar).definitions
}

/// The identifier under `offset`. The one-off form of [`FileSymbols::identifier_at`].
pub fn identifier_at(text: &str, offset: usize, grammar: &Grammar) -> Option<Range<usize>> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    FileSymbols::read(text, grammar).identifier_at(offset)
}

/// Every whole-word occurrence of `name`. The one-off form of [`FileSymbols::occurrences`].
pub fn occurrences(text: &str, name: &str, grammar: &Grammar) -> Vec<Occurrence> {
    FileSymbols::read(text, grammar).occurrences(text, name, grammar)
}

/// Whether the bytes at `start..end` are bounded by something that is not part of a word here.
fn is_whole_word(text: &str, start: usize, end: usize, grammar: &Grammar) -> bool {
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return false;
    }
    let bounded = |character: Option<char>| match character {
        None => true,
        Some(character) => !grammar.is_word_character(character, false),
    };
    bounded(text[..start].chars().next_back()) && bounded(text[end..].chars().next())
}


/// Whether a word directly before `(` is a method being declared rather than one being called.
///
/// The one heuristic in the module, and it exists for the definition Rust never hides but
/// JavaScript and TypeScript do: **a class method has no keyword in front of its name**. The rule
/// is deliberately narrow, because a `Likely` tier that guesses harder stops being honest:
///
/// - not preceded by a `.`, so `list.map(x => {` is a call on something else;
/// - its parameter list closes on the **same line**, so a method whose parameters span lines is
///   missed rather than half-found — which is stated in the plugin's own `plugin.limitations`;
/// - followed by `{`, so `draw(area)` on its own is a call and `if (ready) {` is not reached at all,
///   because `if` is a keyword and never a `Function` token.
fn is_brace_definition(text: &str, name: &Range<usize>) -> bool {
    if text[..name.start].trim_end().ends_with('.') {
        return false;
    }
    let bytes = text.as_bytes();
    let mut at = name.end;
    if bytes.get(at) != Some(&b'(') {
        return false;
    }
    let mut depth = 0_i32;
    while at < bytes.len() {
        match bytes[at] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            // The parameter list has to close on the line it opened on.
            b'\n' => return false,
            _ => {}
        }
        at += 1;
    }
    if depth != 0 || at >= bytes.len() {
        return false;
    }
    // Whatever follows the closing bracket, skipping spaces but not line breaks: a `{` on the next
    // line is a style this rule deliberately does not read, for the same reason as the parameters.
    text[at + 1..].trim_start_matches([' ', '\t']).starts_with('{')
}


/// Everything ranking needs to know about one candidate definition, with no idea what a file is.
///
/// The app has the paths and the index has the definitions; what has to be settled here is the
/// **order**, because that is the part a test can pin down with no disk behind it. `file_order` is
/// the caller's own ordering of its files, and is only ever used to break a tie, so the same
/// question always gets the same answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankKey {
    /// True when this candidate is in the file the question was asked in.
    pub same_file: bool,
    /// Where the candidate's name starts, which is what "nearest above" is measured with.
    pub start: usize,
    pub kind: SymbolKind,
    pub confidence: Confidence,
    /// Which file it is in, in the caller's order.
    pub file_order: usize,
}

/// The order candidates should be offered in, as indices into `keys`.
///
/// 1. Definitions in the **same file**, the nearest one *above* the point first — which is what
///    makes a shadowed local resolve to the nearest `let` above it more often than not, without
///    pretending to scope analysis. A definition below the point comes after every one above it,
///    nearest first.
/// 2. Then definitions in other files: `Sure` before `Likely`, functions and types before
///    constants, modules and variables, then by the caller's file order and by position.
///
/// One candidate means a jump. Several means the modal, because a picker for "which `new` did you
/// mean" and a reference list are the same furniture. Nothing ever silently jumps to a guess when
/// the mechanism knows it guessed.
pub fn rank(keys: &[RankKey], asked_at: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|index| sort_key(&keys[*index], asked_at));
    order
}

/// What one candidate sorts as. Every part is an integer, so the order is total and the same on
/// every machine — which is what the determinism property rests on.
fn sort_key(key: &RankKey, asked_at: usize) -> (u8, u8, usize, u8, u8, usize, usize) {
    let elsewhere = u8::from(!key.same_file);
    // Above the point beats below it, and among those above, the nearest is the one that starts
    // latest, so the distance upwards is what sorts.
    let (side, distance) = match (key.same_file, key.start <= asked_at) {
        (true, true) => (0, asked_at - key.start),
        (true, false) => (1, key.start - asked_at),
        (false, _) => (0, 0),
    };
    let guessed = u8::from(key.confidence == Confidence::Likely);
    let kind = match key.kind {
        SymbolKind::Function => 0,
        SymbolKind::Type => 1,
        SymbolKind::Constant => 2,
        SymbolKind::Module => 3,
        SymbolKind::Variable => 4,
    };
    (elsewhere, side, distance, guessed, kind, key.file_order, key.start)
}

/// Turn a set of ranges and a new name into the edits that apply it, back to front.
///
/// Back to front is the whole of the arithmetic: applied in that order, no edit can shift the
/// range of one still to be made, so a name occurring twice on one line is replaced twice
/// correctly and a replacement longer or shorter than what it replaces needs no bookkeeping at
/// all. Overlapping ranges are dropped rather than applied twice, because two edits over the same
/// bytes have no meaning; ranges outside the text are dropped for the same reason.
pub fn replacements(text: &str, ranges: &[Range<usize>], to: &str) -> Vec<(Range<usize>, String)> {
    let mut sorted: Vec<Range<usize>> = ranges
        .iter()
        .filter(|range| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .cloned()
        .collect();
    sorted.sort_by_key(|range| range.start);
    sorted.dedup();
    let mut out: Vec<(Range<usize>, String)> = Vec::with_capacity(sorted.len());
    let mut reached = 0;
    for range in sorted {
        if range.start < reached {
            continue;
        }
        reached = range.end;
        out.push((range, to.to_owned()));
    }
    out.reverse();
    out
}

/// The text `edits` produce, worked out independently of the code that applies them.
///
/// This is what the non-destruction property test compares against: a rename must equal the input
/// with exactly the chosen ranges substituted and every other byte untouched, and a test that
/// checked the applier against itself would prove nothing. It is also how a closed file is
/// rewritten, since there is no `Document` behind one.
pub fn applied(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut ordered: Vec<&(Range<usize>, String)> = edits.iter().collect();
    ordered.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, replacement) in ordered {
        if range.start < at || range.end > text.len() {
            continue;
        }
        out.push_str(&text[at..range.start]);
        out.push_str(replacement);
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Whether `name` is a word this grammar could hold, and why not when it could not.
///
/// A rename is refused rather than applied when the answer is `Err`, because a new name that is a
/// keyword or that is not one word would not be the same identifier afterwards — it would be a
/// syntax error somebody has to find by compiling.
pub fn check_name(name: &str, grammar: &Grammar) -> Result<(), String> {
    if name.is_empty() {
        return Err("A name cannot be empty.".to_owned());
    }
    let mut characters = name.chars();
    let first = characters.next().unwrap_or(' ');
    if !grammar.is_word_character(first, true) {
        return Err(format!("'{name}' cannot start with '{first}'."));
    }
    for character in characters {
        if !grammar.is_word_character(character, false) {
            return Err(format!("'{name}' cannot hold '{character}'."));
        }
    }
    if grammar.keywords.iter().any(|keyword| keyword == name) {
        let language = if grammar.language.is_empty() { "reserved" } else { grammar.language.as_str() };
        return Err(format!("'{name}' is a {language} keyword."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Rust grammar as the bundled plugin describes it, cut down to what these tests need.
    fn rust() -> Grammar {
        let words = |list: &str| list.split(' ').map(str::to_owned).collect::<Vec<String>>();
        Grammar {
            language: "Rust".to_owned(),
            keywords: words(
                "fn let mut const static struct enum trait impl type mod use pub crate self where for in while loop if else match return break continue as dyn",
            ),
            builtins: words("String Vec Option Result Some None Ok Err usize"),
            line_comment: Some("//".to_owned()),
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"'],
            escapes: true,
            operators: "+-*/%=<>!&|^?:;,.#".chars().collect(),
            numbers: true,
            definers: vec![
                ("fn".to_owned(), SymbolKind::Function),
                ("struct".to_owned(), SymbolKind::Type),
                ("enum".to_owned(), SymbolKind::Type),
                ("trait".to_owned(), SymbolKind::Type),
                ("mod".to_owned(), SymbolKind::Module),
                ("const".to_owned(), SymbolKind::Constant),
                ("static".to_owned(), SymbolKind::Constant),
                ("type".to_owned(), SymbolKind::Type),
                ("let".to_owned(), SymbolKind::Variable),
            ],
            ..Grammar::default()
        }
    }

    /// TypeScript, which is the language the brace heuristic exists for.
    fn typescript() -> Grammar {
        let words = |list: &str| list.split(' ').map(str::to_owned).collect::<Vec<String>>();
        Grammar {
            language: "TypeScript".to_owned(),
            keywords: words(
                "const let var function class interface type enum namespace return if else for while switch case try catch new extends async await import export from as static this",
            ),
            builtins: words("console Promise Array Object Math JSON string number boolean"),
            line_comment: Some("//".to_owned()),
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"', '\'', '`'],
            escapes: true,
            operators: "+-*/%=<>!&|^~?:;,.@".chars().collect(),
            numbers: true,
            definers: vec![
                ("function".to_owned(), SymbolKind::Function),
                ("class".to_owned(), SymbolKind::Type),
                ("interface".to_owned(), SymbolKind::Type),
                ("enum".to_owned(), SymbolKind::Type),
                ("type".to_owned(), SymbolKind::Type),
                ("namespace".to_owned(), SymbolKind::Module),
                ("const".to_owned(), SymbolKind::Variable),
                ("let".to_owned(), SymbolKind::Variable),
                ("var".to_owned(), SymbolKind::Variable),
            ],
            brace_definitions: true,
            ..Grammar::default()
        }
    }

    /// CSS, which deliberately has no definers at all.
    fn css() -> Grammar {
        Grammar {
            language: "CSS".to_owned(),
            keywords: vec!["@media".to_owned()],
            builtins: vec!["background-color".to_owned()],
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"'],
            operators: "{}();:,".chars().collect(),
            numbers: true,
            word_characters: vec!['-', '@'],
            hex_colors: true,
            ..Grammar::default()
        }
    }

    /// The definitions in a piece of source, as `(name, kind, confidence)`, so a test reads like
    /// the thing it is about.
    fn definitions(text: &str, grammar: &Grammar) -> Vec<(String, SymbolKind, Confidence)> {
        file_definitions(text, grammar)
            .into_iter()
            .map(|definition| {
                (text[definition.name_range].to_owned(), definition.kind, definition.confidence)
            })
            .collect()
    }

    fn names(text: &str, grammar: &Grammar) -> Vec<String> {
        definitions(text, grammar).into_iter().map(|(name, _, _)| name).collect()
    }

    #[test]
    fn a_definer_keyword_followed_by_a_word_is_a_definition() {
        let text = "fn draw(area: Rect) {}\nstruct Layout {}\nconst LIMIT: usize = 4;\n";
        let found = definitions(text, &rust());
        assert!(
            found.contains(&("draw".to_owned(), SymbolKind::Function, Confidence::Sure)),
            "{found:?}"
        );
        assert!(
            found.contains(&("Layout".to_owned(), SymbolKind::Type, Confidence::Sure)),
            "{found:?}"
        );
        assert!(
            found.contains(&("LIMIT".to_owned(), SymbolKind::Constant, Confidence::Sure)),
            "{found:?}"
        );
    }

    #[test]
    fn a_keyword_the_language_did_not_name_is_stepped_over() {
        // `let mut count` and `pub const LIMIT` are ordinary, and the word after the second keyword
        // is still the name.
        assert!(names("let mut count = 0;", &rust()).contains(&"count".to_owned()));
        assert!(names("pub const LIMIT: usize = 4;", &rust()).contains(&"LIMIT".to_owned()));
        // And a second definer replaces the first, so this defines a function and not a constant.
        let found = definitions("const fn new() -> Self {}", &rust());
        assert_eq!(found[0].0, "new");
        assert_eq!(found[0].1, SymbolKind::Function);
    }

    #[test]
    fn a_run_ends_at_an_operator_or_a_line_break() {
        // `let (a, b) = f()` defines nothing rather than defining whatever word comes next.
        assert!(names("let (a, b) = split();", &rust()).is_empty(), "a pattern is not a name");
        // And a name the language provides is not one this file is declaring.
        assert!(names("let Some(value) = maybe;", &rust()).is_empty(), "`Some` is a builtin");
        // A definer at the end of a line does not reach across it.
        assert!(names("let\nvalue = 1;", &rust()).is_empty());
    }

    #[test]
    fn nothing_inside_a_comment_or_a_string_is_a_definition() {
        // The tokeniser already said what those are, so the pass does not have to be clever.
        assert!(names("// fn draw() {}\n", &rust()).is_empty());
        let found = names("let s = \"fn draw\";", &rust());
        assert_eq!(found, vec!["s".to_owned()], "only the `s`");
    }

    #[test]
    fn a_class_method_is_found_by_its_shape_and_is_marked_as_a_guess() {
        // Scenario 9. The definition Rust never hides but JavaScript and TypeScript do.
        let source = "class Panel {\n  render(area) {\n    return area;\n  }\n}\n";
        let found = definitions(source, &typescript());
        assert!(
            found.contains(&("Panel".to_owned(), SymbolKind::Type, Confidence::Sure)),
            "{found:?}"
        );
        assert!(
            found.contains(&("render".to_owned(), SymbolKind::Function, Confidence::Likely)),
            "the method is found, and it is marked as a guess: {found:?}"
        );
    }

    #[test]
    fn the_shapes_the_brace_rule_must_not_catch() {
        // Scenario 10, one line each, and each for its own reason.
        assert!(names("if (ready) {\n}\n", &typescript()).is_empty(), "`if` is a keyword");
        assert!(names("list.map(x => {\n});\n", &typescript()).is_empty(), "`map` follows a dot");
        assert!(names("draw(area);\n", &typescript()).is_empty(), "no brace follows the call");
        // And a method whose parameters span lines is missed rather than half-found.
        assert!(names("render(\n  area\n) {\n}\n", &typescript()).is_empty());
    }

    #[test]
    fn a_language_that_asks_for_neither_produces_no_definitions_at_all() {
        // Scenario 16: CSS defines a custom property by position rather than by keyword, and a rule
        // that read `:` as a definer would call every property a definition.
        let text = "--brand-hue: 280;\n.card { background-color: #ff79c6; }\n";
        assert!(file_definitions(text, &css()).is_empty());
        assert!(file_definitions(text, &Grammar::default()).is_empty());
        assert!(!css().defines_symbols(), "which is what the menu asks before it draws an entry");
    }

    #[test]
    fn the_word_under_a_point_is_grown_in_both_directions() {
        let text = "let value = other;";
        assert_eq!(identifier_at(text, 5, &rust()), Some(4..9), "the middle of `value`");
        assert_eq!(identifier_at(text, 4, &rust()), Some(4..9), "its first byte");
        assert_eq!(identifier_at(text, 9, &rust()), Some(4..9), "and the caret just after it");
    }

    #[test]
    fn a_point_that_is_not_a_question_about_a_symbol_answers_nothing() {
        // Scenario 5: a keyword, a number, an operator, inside a comment, inside a string.
        let text = "let x = 42; // let y\nlet s = \"let z\";";
        assert_eq!(identifier_at(text, 1, &rust()), None, "inside `let`");
        assert_eq!(identifier_at(text, 9, &rust()), None, "inside `42`");
        assert_eq!(identifier_at(text, 6, &rust()), None, "on the `=`");
        assert_eq!(identifier_at(text, 16, &rust()), None, "inside the comment");
        let quote = text.find("\"let z\"").expect("the string");
        assert_eq!(identifier_at(text, quote + 2, &rust()), None, "inside the string");
    }

    #[test]
    fn the_first_and_last_bytes_of_a_file_are_still_words() {
        // Scenario 15.
        let text = "value = other";
        assert_eq!(identifier_at(text, 0, &rust()), Some(0..5));
        assert_eq!(identifier_at(text, text.len() - 1, &rust()), Some(8..13));
        assert_eq!(identifier_at(text, text.len(), &rust()), Some(8..13));
        assert_eq!(identifier_at("", 0, &rust()), None);
    }

    #[test]
    fn a_hyphenated_css_property_is_one_identifier_and_a_rust_one_is_two() {
        // The whole reason `is_word_character` is asked rather than assumed.
        assert_eq!(identifier_at("--brand-hue: 280;", 4, &css()), Some(0..11));
        assert_eq!(identifier_at("mid - word", 1, &rust()), Some(0..3), "and `mid` is its own");
    }

    #[test]
    fn an_occurrence_is_a_whole_word_and_nothing_less() {
        // Scenario 23.
        let text = "let count = 1;\nlet counter = count + 1;\nlet x = 2; let x2 = 3;";
        let found = occurrences(text, "count", &rust());
        assert_eq!(found.len(), 2, "`counter` is not a `count`: {found:?}");
        for occurrence in &found {
            assert_eq!(&text[occurrence.range.clone()], "count");
        }
        assert_eq!(occurrences(text, "x", &rust()).len(), 1, "`x2` is not an `x`");
    }

    #[test]
    fn an_occurrence_carries_the_role_of_what_it_was_found_inside() {
        // Scenario 24. Shown, because a rename that must update a doc comment needs to find it;
        // told apart, because they are textual matches and the modal does not pretend otherwise.
        let text = "// draw the thing\nfn draw() {}\nlet s = \"draw\";\n";
        let found = occurrences(text, "draw", &rust());
        let roles: Vec<Role> = found.iter().map(|occurrence| occurrence.role).collect();
        assert_eq!(roles, vec![Role::Comment, Role::Code, Role::String], "{found:?}");
    }

    #[test]
    fn a_name_that_is_not_there_is_no_occurrences_rather_than_a_panic() {
        assert!(occurrences("let a = 1;", "missing", &rust()).is_empty());
        assert!(occurrences("", "a", &rust()).is_empty());
        assert!(occurrences("aaa", "", &rust()).is_empty());
    }

    #[test]
    fn letters_wider_than_one_byte_land_on_character_boundaries() {
        // Scenario 14. The ranges have to be usable as byte ranges into the same text.
        let text = "let d\u{00E9}j\u{00E0} = 1;\nlet s = \"\u{1F600}\";\nd\u{00E9}j\u{00E0} + 1;\n";
        let found = occurrences(text, "d\u{00E9}j\u{00E0}", &rust());
        assert_eq!(found.len(), 2, "{found:?}");
        for occurrence in &found {
            assert!(text.is_char_boundary(occurrence.range.start));
            assert!(text.is_char_boundary(occurrence.range.end));
            assert_eq!(&text[occurrence.range.clone()], "d\u{00E9}j\u{00E0}");
        }
        assert_eq!(definitions(text, &rust())[0].0, "d\u{00E9}j\u{00E0}");
    }

    #[test]
    fn the_nearest_definition_above_the_point_is_offered_first() {
        // Scenario 4: `let x` at two places, asked about below both of them.
        let keys = [
            RankKey {
                same_file: true,
                start: 10,
                kind: SymbolKind::Variable,
                confidence: Confidence::Sure,
                file_order: 0,
            },
            RankKey {
                same_file: true,
                start: 90,
                kind: SymbolKind::Variable,
                confidence: Confidence::Sure,
                file_order: 0,
            },
        ];
        assert_eq!(rank(&keys, 200), vec![1, 0], "the later one is the nearer one above");
        assert_eq!(rank(&keys, 50), vec![0, 1], "asked between them, the one above wins");
    }

    #[test]
    fn this_file_beats_another_and_a_sure_answer_beats_a_guess() {
        let here = RankKey {
            same_file: true,
            start: 5,
            kind: SymbolKind::Variable,
            confidence: Confidence::Sure,
            file_order: 3,
        };
        let sure = RankKey {
            same_file: false,
            start: 0,
            kind: SymbolKind::Function,
            confidence: Confidence::Sure,
            file_order: 1,
        };
        let guess = RankKey {
            same_file: false,
            start: 0,
            kind: SymbolKind::Function,
            confidence: Confidence::Likely,
            file_order: 0,
        };
        assert_eq!(rank(&[sure, guess, here], 100), vec![2, 0, 1]);
    }

    #[test]
    fn ranking_the_same_candidates_twice_gives_the_same_order() {
        // Scenario 19's determinism, at the ranking end: ties are settled by the caller's file
        // order and by position, both integers, so there is nothing left to be arbitrary.
        let keys: Vec<RankKey> = (0..8)
            .map(|index| RankKey {
                same_file: false,
                start: 40,
                kind: SymbolKind::Function,
                confidence: Confidence::Sure,
                file_order: index,
            })
            .collect();
        assert_eq!(rank(&keys, 0), rank(&keys, 0));
        assert_eq!(rank(&keys, 0), (0..8).collect::<Vec<usize>>());
    }

    #[test]
    fn replacements_are_ordered_back_to_front_so_no_range_shifts_another() {
        let text = "draw(); draw(); draw();";
        let ranges = ranges_of(text, "draw", &rust());
        let edits = replacements(text, &ranges, "paint");
        assert_eq!(edits.len(), 3);
        assert!(edits[0].0.start > edits[1].0.start, "the last one is applied first");
        assert_eq!(applied(text, &edits), "paint(); paint(); paint();");
    }

    #[test]
    fn a_name_that_occurs_twice_on_one_line_is_replaced_twice() {
        // Scenario 45.
        let text = "total = total + 1;";
        let edits = replacements(text, &ranges_of(text, "total", &rust()), "count");
        assert_eq!(applied(text, &edits), "count = count + 1;");
    }

    #[test]
    fn a_replacement_longer_shorter_and_the_same_length_all_come_out_right() {
        // Scenario 46, the classic off-by-one family, in one test.
        let text = "let value = value + value;";
        let ranges = ranges_of(text, "value", &rust());
        assert_eq!(ranges.len(), 3);
        assert_eq!(applied(text, &replacements(text, &ranges, "v")), "let v = v + v;");
        assert_eq!(
            applied(text, &replacements(text, &ranges, "total")),
            "let total = total + total;"
        );
        assert_eq!(
            applied(text, &replacements(text, &ranges, "measurement")),
            "let measurement = measurement + measurement;"
        );
    }

    #[test]
    fn only_the_chosen_ranges_change_and_every_other_byte_is_untouched() {
        // Scenario 37 and the non-destruction property: two of three occurrences chosen.
        let text = "let value = value + value;";
        let ranges = ranges_of(text, "value", &rust());
        let chosen = vec![ranges[0].clone(), ranges[2].clone()];
        assert_eq!(
            applied(text, &replacements(text, &chosen, "total")),
            "let total = value + total;"
        );
    }

    #[test]
    fn the_line_endings_of_a_file_survive_a_rename_byte_for_byte() {
        // Scenario 48. Bytes outside the replaced ranges are untouched, so a file written with
        // carriage returns keeps every one of them.
        let text = "let value = 1;\r\nlet other = value;\r\n";
        let edits = replacements(text, &ranges_of(text, "value", &rust()), "total");
        let after = applied(text, &edits);
        assert_eq!(after.matches("\r\n").count(), 2);
        assert_eq!(after, "let total = 1;\r\nlet other = total;\r\n");
    }

    #[test]
    fn a_range_that_overlaps_another_or_runs_past_the_end_is_dropped() {
        let text = "abcdef";
        let edits = replacements(text, &[0..3, 1..4], "X");
        assert_eq!(edits.len(), 1, "two edits over the same bytes have no meaning: {edits:?}");
        assert!(replacements(text, &[10..20], "X").is_empty());
        assert!(replacements(text, &[3..3], "X").is_empty(), "an empty range replaces nothing");
    }

    #[test]
    fn a_new_name_has_to_be_a_word_of_this_language() {
        // Scenario 38, and the reason is that the alternative is a syntax error somebody has to
        // find by compiling.
        assert!(check_name("total", &rust()).is_ok());
        assert!(check_name("", &rust()).is_err());
        assert!(check_name("two words", &rust()).unwrap_err().contains(' '));
        assert!(check_name("9lives", &rust()).is_err(), "a name cannot start with a digit");
        let refusal = check_name("match", &rust()).expect_err("a keyword is not a name");
        assert!(refusal.contains("match") && refusal.contains("Rust"), "{refusal}");
        // And a language that says a hyphen is a letter accepts one.
        assert!(check_name("brand-hue", &css()).is_ok());
        assert!(check_name("brand-hue", &rust()).is_err());
    }

    #[test]
    fn which_kinds_rename_the_whole_project_by_default() {
        // The table in the TDD's §6.1: a variable is scoped to its own file, everything else is not.
        assert!(!SymbolKind::Variable.renames_the_project());
        for kind in [
            SymbolKind::Function,
            SymbolKind::Type,
            SymbolKind::Constant,
            SymbolKind::Module,
        ] {
            assert!(kind.renames_the_project(), "{kind:?}");
        }
    }

    #[test]
    fn a_kind_is_read_from_a_manifest_and_an_unknown_one_is_refused() {
        for kind in SymbolKind::ALL {
            assert_eq!(SymbolKind::parse(kind.name()), Some(kind));
        }
        assert_eq!(SymbolKind::parse("gadget"), None);
    }

    #[test]
    fn reading_a_file_once_answers_exactly_as_asking_it_three_times_does() {
        // The whole reason `FileSymbols` may exist: it is the same answers, kept. A form that read
        // the file once and drifted from the free functions would be a hover that underlined a word
        // the menu then said it had never heard of.
        for (text, grammar) in fixtures() {
            let read = FileSymbols::read(&text, &grammar);
            assert_eq!(read.definitions(), file_definitions(&text, &grammar));
            for name in ["value", "draw", "count"] {
                assert_eq!(
                    read.occurrences(&text, name, &grammar),
                    occurrences(&text, name, &grammar)
                );
            }
            for offset in 0..=text.len() {
                if !text.is_char_boundary(offset) {
                    continue;
                }
                assert_eq!(
                    read.identifier_at(offset),
                    identifier_at(&text, offset, &grammar),
                    "at {offset} of {text:?}"
                );
            }
        }
    }

    #[test]
    fn the_distinct_words_of_a_file_are_each_spelling_once_in_order() {
        // What completion offers for the names no definer keyword can see. A file naming `value`
        // five times offers it once, and the list is sorted so it is the same on every run.
        let text = "fn draw(value: usize) -> usize {
    let count = value;
    count + value
}
";
        let read = FileSymbols::read(text, &rust());
        let words = read.distinct_words(text);
        assert_eq!(words, vec!["count", "draw", "usize", "value"], "{words:?}");
        assert!(read.words() > words.len(), "the file says `value` more than once");
        assert_eq!(words, read.distinct_words(text), "and the same list every time");
        // A language with no definers at all still has words, which is the whole point of them.
        let css_text = ".card { --brand-hue: 280; background-color: #ff79c6; }
";
        let css_words = FileSymbols::read(css_text, &css()).distinct_words(css_text);
        assert!(css_words.contains(&"--brand-hue".to_owned()), "{css_words:?}");
        assert!(css_words.contains(&"background-color".to_owned()), "{css_words:?}");
    }

    #[test]
    fn a_file_read_once_holds_its_words_in_order_so_a_hover_is_a_binary_search() {
        let text = "fn draw(area: Rect) {\n    let count = area;\n}\n";
        let read = FileSymbols::read(text, &rust());
        assert!(read.words() >= 4, "draw, area, Rect, count at least");
        // Every word answers about its own first byte, which is what the binary search rests on.
        for offset in 0..text.len() {
            if let Some(word) = read.identifier_at(offset) {
                assert!(word.start <= offset && offset <= word.end);
                assert!(!text[word].chars().any(char::is_whitespace));
            }
        }
    }

    /// **Truthfulness**: every range any of these functions returns lies inside the text it came
    /// from, on character boundaries, and the text at a definition's range is the definition's own
    /// name.
    #[test]
    fn every_range_is_inside_the_text_and_on_a_character_boundary() {
        for (text, grammar) in fixtures() {
            for definition in file_definitions(&text, &grammar) {
                let range = definition.name_range;
                assert!(range.end <= text.len(), "{range:?} runs past the end");
                assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
                let name = &text[range.clone()];
                assert!(!name.is_empty(), "a definition with no name at {range:?}");
                assert!(
                    !name.chars().any(char::is_whitespace),
                    "a definition's range is one word: {name:?}"
                );
                // And asking about the definition's own first byte gives that same word back.
                assert_eq!(identifier_at(&text, range.start, &grammar), Some(range));
            }
            for occurrence in occurrences(&text, "value", &grammar) {
                assert_eq!(&text[occurrence.range], "value");
            }
        }
    }

    /// **Determinism**: the same text and the same grammar give identical answers every time.
    #[test]
    fn reading_the_same_source_twice_gives_an_identical_answer() {
        for (text, grammar) in fixtures() {
            assert_eq!(file_definitions(&text, &grammar), file_definitions(&text, &grammar));
            assert_eq!(occurrences(&text, "value", &grammar), occurrences(&text, "value", &grammar));
            assert_eq!(occurrences(&text, "draw", &grammar), occurrences(&text, "draw", &grammar));
        }
    }

    /// **Non-destruction**: applying a rename to any of the fixtures equals the input with exactly
    /// those ranges substituted, reconstructed here rather than taken from the applier.
    #[test]
    fn a_rename_changes_the_chosen_ranges_and_nothing_else() {
        for (text, grammar) in fixtures() {
            for name in ["value", "draw", "count"] {
                let ranges = ranges_of(&text, name, &grammar);
                let after = applied(&text, &replacements(&text, &ranges, "renamed"));
                // Rebuilt independently: everything outside the ranges, in order, unchanged.
                let mut expected = String::new();
                let mut at = 0;
                for range in &ranges {
                    expected.push_str(&text[at..range.start]);
                    expected.push_str("renamed");
                    at = range.end;
                }
                expected.push_str(&text[at..]);
                assert_eq!(after, expected, "renaming {name} in {text:?}");
                assert_eq!(
                    after.len(),
                    text.len() + ranges.len() * ("renamed".len() - name.len())
                );
            }
        }
    }

    /// **Isolation**: nothing in this module reads or writes anything. There is no filesystem call
    /// in it to test for, so what is checked is that every entry point answers from a `&str` and a
    /// grammar alone — which is what the signatures say and what this exercises with no path in
    /// sight.
    #[test]
    fn every_answer_comes_from_the_text_and_the_grammar_alone() {
        let grammar = rust();
        let text = "fn draw() {}\n";
        assert!(!file_definitions(text, &grammar).is_empty());
        assert!(identifier_at(text, 3, &grammar).is_some());
        assert!(!occurrences(text, "draw", &grammar).is_empty());
        assert!(check_name("ok", &grammar).is_ok());
    }

    fn ranges_of(text: &str, name: &str, grammar: &Grammar) -> Vec<Range<usize>> {
        occurrences(text, name, grammar).into_iter().map(|found| found.range).collect()
    }

    /// The sources every property is held against: a little of each language, including the
    /// awkward shapes.
    fn fixtures() -> Vec<(String, Grammar)> {
        vec![
            (
                "fn draw(value: usize) -> usize {\n    let count = value;\n    // draw the value\n    count\n}\nstruct Value;\n".to_owned(),
                rust(),
            ),
            (
                "class Panel {\n  render(value) {\n    const count = value;\n    return this.draw(count);\n  }\n  draw(value) { return value; }\n}\n".to_owned(),
                typescript(),
            ),
            (
                ".card { --value: 4px; background-color: #ff79c6; }\n/* the value */\n".to_owned(),
                css(),
            ),
            (String::new(), rust()),
            ("value".to_owned(), rust()),
            ("value value value".to_owned(), rust()),
            ("let d\u{00E9}j\u{00E0} = value;\n".to_owned(), rust()),
        ]
    }
}
