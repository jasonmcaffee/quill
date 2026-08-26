//! Offering a name while it is still being typed.
//!
//! [`crate::symbols`] answers "where is this name defined and used" once the name exists; this
//! module is the other half of the same question, asked one letter earlier: *given what has been
//! typed so far, which names could it become*. Like the module beside it, it is pure — it reads a
//! `&str` and a [`Grammar`], it draws nothing, it touches no disk, and its tests run with no
//! window.
//!
//! ## What it decides, and what it does not
//!
//! It decides three things and nothing else: what the **stem** under the caret is, which of a pile
//! of [`Candidate`]s that stem **matches**, and what **order** the matches are offered in. Where
//! the candidates came from is the window's business — `app/completion.rs` gathers them from the
//! open tabs, the project's index and the grammar — and drawing them is the popup's.
//!
//! ## The match, and why it is a subsequence
//!
//! **A candidate matches when the stem is a case-insensitive subsequence of it.** `lyt` finds
//! `layout`, `psttx` finds `paint_text`, and middle matching comes free, so `draw` finds `redraw`.
//! That is IntelliJ's documented behaviour and Sublime Text's, and it is the shape
//! [`crate::symbols`]' sibling `services::file_search` already ranks file names by.
//!
//! ## The score, and why the tests pin orderings rather than numbers
//!
//! The rubric is Sublime Text's, restated for identifiers: a large bonus when the candidate starts
//! with the stem, a bonus per matched letter sitting on a word boundary, a bonus per consecutive
//! matched letter, a small bonus per letter whose case agrees exactly, and a penalty per unmatched
//! letter so the shorter of two otherwise-equal names wins.
//!
//! The alignment behind a score is the **best** one rather than the first, found by dynamic
//! programming over the two strings, because `pt` has two readings of `paint_text` and only one of
//! them is the one a person meant. But a score is meaningless outside a comparison, so every test
//! here pins an **order**: a test asserting `-13` would be a test of the constants rather than of
//! anything anybody can see.
//!
//! ## And why the order is total
//!
//! Ties are broken by source, then by the shorter name, then by the name's own bytes, so the same
//! text and the same stem give the same list in the same order every time. That is not tidiness:
//! the popup's screenshot tests and the command line's output both rest on it.

use std::ops::Range;

use crate::symbols::SymbolKind;
use crate::syntax::Grammar;

/// A large bonus for a candidate that **starts with** the stem, which is by far the commonest
/// intent: somebody typing `dra` nearly always wants `draw` rather than `redraw`.
const PREFIX: i32 = 30;
/// A matched letter sitting at the start of the name or of a part of it — after `_` or `-`, or at a
/// lower-to-upper camel step. Worth the most of the per-letter bonuses, which is what makes `pt`
/// prefer `paint_text` over `pointer`.
const BOUNDARY: i32 = 12;
/// A matched letter directly after another matched letter.
const CONSECUTIVE: i32 = 6;
/// A matched letter whose case agrees with what was typed. Small, because a bonus that decided
/// anything on its own would make completion case-sensitive by the back door.
const SAME_CASE: i32 = 2;
/// Per letter of the candidate the stem did not match, so the shorter of two otherwise-equal names
/// is offered first.
const UNMATCHED: i32 = 1;

/// A score no alignment can reach, standing for "these two do not line up at all".
const IMPOSSIBLE: i32 = i32::MIN / 4;

/// Where a candidate came from.
///
/// It is carried all the way to the row, because a row that can say `draw_frame · layout.rs` is
/// answering "why is this being offered" and a bare word cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// A definition in the file being typed in.
    ThisFile,
    /// A word of the file being typed in — a local, a parameter, a field name, a CSS property.
    /// Everything the definers cannot see, and the only source a language with no definers has.
    Word,
    /// A definition in another tab that is open, read from its live text.
    OpenTab,
    /// A definition the project's index holds, which is every file that is **not** open. The
    /// ownership rule of `task-1675` §3.3: a file that is open is owned by its `Document`.
    Index,
    /// One of the language's own words: a keyword, a builtin or a type from the manifest.
    Language,
    /// A file or a module, offered inside an import. `task-1680`'s one new source: the rows a
    /// specifier or a module path could become, which are not names inside a file but files.
    Module,
}

impl Source {
    /// Where this source comes in the offered order, which is only ever used to break a tie between
    /// two equally-scored rows.
    ///
    /// The nearest answer first: what this file defines, then what this file says, then the other
    /// tabs, then the disk, then the language itself — which is last because a keyword is the one
    /// candidate a person can always type out from memory.
    /// A module comes first, which only ever decides a tie and only inside an import, where a
    /// module and an item of the same name can both be offered. The module wins it, because
    /// `use a::b` with `b` both a module and a function far more often means the module.
    pub fn order(self) -> u8 {
        match self {
            Source::Module => 0,
            Source::ThisFile => 1,
            Source::Word => 2,
            Source::OpenTab => 3,
            Source::Index => 4,
            Source::Language => 5,
        }
    }

    /// Which source wins the row when two of them offer the same spelling.
    ///
    /// A different order from [`Self::order`], and deliberately: **a definition beats a keyword
    /// beats a plain word**, because a definition has the most to say about itself and a plain word
    /// has nothing at all. Offering is about how near the answer is; labelling is about how much
    /// the answer knows.
    fn describes_itself(self) -> u8 {
        match self {
            Source::Module => 0,
            Source::ThisFile => 1,
            Source::OpenTab => 2,
            Source::Index => 3,
            Source::Language => 4,
            Source::Word => 5,
        }
    }

    /// The word the command line prints and a test compares against.
    pub fn name(self) -> &'static str {
        match self {
            Source::ThisFile => "this file",
            Source::Word => "word",
            Source::OpenTab => "open tab",
            Source::Index => "project",
            Source::Language => "language",
            Source::Module => "module",
        }
    }
}

/// One thing that could be offered, before anything has been matched against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// What would be inserted. The whole of the row's identity: the pool is deduplicated by this,
    /// because two entries that would type the same bytes are one offer.
    pub name: String,
    pub source: Source,
    /// What the definition names, where the candidate is one. Nothing for a word or a keyword.
    pub kind: Option<SymbolKind>,
    /// The quiet suffix a row shows — the defining file's name, or `keyword`. Empty where the
    /// candidate needs no explanation, which is what this file's own words need.
    pub detail: String,
}

impl Candidate {
    /// A candidate with nothing to say about itself but its name, which is what a word is.
    pub fn new(name: impl Into<String>, source: Source) -> Self {
        Self { name: name.into(), source, kind: None, detail: String::new() }
    }

    /// The same, carrying what it is and where it came from.
    pub fn described(
        name: impl Into<String>,
        source: Source,
        kind: Option<SymbolKind>,
        detail: impl Into<String>,
    ) -> Self {
        Self { name: name.into(), source, kind, detail: detail.into() }
    }
}

/// One offered row: a candidate that matched, with its score and which of its letters matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub source: Source,
    pub kind: Option<SymbolKind>,
    pub detail: String,
    /// Which **characters** of the name the stem landed on, in order.
    ///
    /// Characters rather than bytes, because that is what picking letters out of a drawn string
    /// counts in — `components::controls::marked_text` walks the name a character at a time — and
    /// because a byte position would be a different number in `déjà` depending on the accents
    /// before it.
    pub matched: Vec<usize>,
    pub score: i32,
}

/// The identifier characters immediately left of the caret: what has been typed of the word so far.
///
/// Empty when there are none, which is what the automatic trigger reads as "there is no word being
/// typed here" and what the manual one reports as an honest miss.
///
/// The characters are the grammar's own, which is the whole reason this asks rather than assumes: a
/// hyphen bounds a word in Rust and is inside one in CSS, so `--brand-hue` is one stem there and
/// three words here. And the **first** character has to be one a word may start with, so the caret
/// after `42` has no stem at all rather than a stem of `42` that matches nothing.
pub fn stem_at(text: &str, offset: usize, grammar: &Grammar) -> Range<usize> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return 0..0;
    }
    let mut start = offset;
    for (index, character) in text[..offset].char_indices().rev() {
        if !grammar.is_word_character(character, false) {
            break;
        }
        start = index;
    }
    while start < offset {
        let character = text[start..].chars().next().expect("start is inside the text");
        if grammar.is_word_character(character, true) {
            break;
        }
        start += character.len_utf8();
    }
    start..offset
}

/// The whole identifier the caret is inside: the stem, and whatever is still to the right of it.
///
/// What `Tab` replaces. `dra│wing` completed to `draw_frame` should not leave `wing` dangling
/// behind the caret, which is IntelliJ's own reason for having two acceptance keys.
pub fn word_at(text: &str, offset: usize, grammar: &Grammar) -> Range<usize> {
    let stem = stem_at(text, offset, grammar);
    if offset > text.len() || !text.is_char_boundary(offset) {
        return stem;
    }
    let mut end = offset;
    for (index, character) in text[offset..].char_indices() {
        if !grammar.is_word_character(character, false) {
            break;
        }
        end = offset + index + character.len_utf8();
    }
    stem.start..end
}

/// Whether a stem could match a name at all, which is the cheap half of the match.
///
/// The one part of scoring a caller needs **before** it decides a candidate is worth building. The
/// window gathers from four thousand names in the project's index a keystroke, and turning each of
/// them into a [`Candidate`] — a string copy, a path's file name, a hash probe for its definition —
/// to have nearly all of them thrown out again is the difference between a keystroke that costs
/// nothing and one that allocates. Answering from two `&str`s costs one walk of each.
pub fn could_match(stem: &str, name: &str) -> bool {
    if stem.is_empty() {
        return false;
    }
    let mut wanted = stem.chars().map(lower).peekable();
    for letter in name.chars().map(lower) {
        if wanted.peek() == Some(&letter) {
            wanted.next();
        }
    }
    wanted.peek().is_none()
}

/// Which rows a stem offers, best first.
///
/// The whole of the module's work in one function: drop what does not match, drop the row equal to
/// the stem, score what is left, keep one row per spelling, and sort into a total order.
///
/// The row **equal to the stem** is dropped rather than offered, which is what makes `Enter` safe
/// once a word is completely typed: with nothing longer to offer the popup has already closed, so
/// `Enter` means the new line the person meant. VS Code grew a three-way setting for the same trap;
/// dropping the no-op row answers it at candidate time instead, and also stops the list offering a
/// row that would do nothing.
pub fn rank(stem: &str, candidates: Vec<Candidate>) -> Vec<Row> {
    if stem.is_empty() {
        return Vec::new();
    }
    rank_all(stem, candidates)
}

/// The same, except that an **empty** stem offers everything rather than nothing.
///
/// `task-1680`. [`rank`]'s guard is right for a word being typed — with nothing typed there is
/// nothing being completed, and a list that opened on every space would be unusable — and wrong
/// for an import, where `from '│'` and `use │` are positions at which the language itself says
/// what comes next, so a list is an answer rather than an interruption. IntelliJ opens its own
/// popup at zero characters after a `.` and after `import` for the same reason.
///
/// With nothing typed nothing can be scored, so the rows come back in the tie-break's own order:
/// by source, then by the shorter name, then by the name's bytes. Which is still total, so the
/// determinism the popup's pictures rest on holds here too.
pub fn rank_all(stem: &str, candidates: Vec<Candidate>) -> Vec<Row> {
    if stem.is_empty() {
        return everything(candidates);
    }
    let needle: Vec<char> = stem.chars().collect();
    let lowered: Vec<char> = needle.iter().flat_map(|c| c.to_lowercase()).collect();
    // The stem's own letters, folded once. A stem whose case folding changes its length — the
    // Turkish dotted capital, and a handful like it — is compared unfolded, because a subsequence
    // of characters is only meaningful while one character stays one character.
    let folded = (lowered.len() == needle.len()).then_some(lowered);
    let folded = folded.as_deref().unwrap_or(&needle);
    let mut scratch = Scratch::default();
    // One row per spelling, chosen as the pool is walked rather than swept up afterwards. Looking a
    // name up in a table rather than searching the rows already kept is not tidiness: a stem of one
    // letter on this repository's largest file offers well over two thousand rows, and the search
    // that was here first compared several million pairs of strings to find that out.
    let mut seen: std::collections::HashMap<String, usize> =
        std::collections::HashMap::with_capacity(candidates.len());
    let mut rows: Vec<Row> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if candidate.name == stem || candidate.name.is_empty() {
            continue;
        }
        // Two entries that would type the same bytes are one offer, and they always score the same,
        // because the score is a function of the name. So a spelling already seen is not scored
        // again — what is being chosen is only the label, and `describes_itself` chooses it.
        if let Some(at) = seen.get(&candidate.name) {
            let known: &mut Row = &mut rows[*at];
            if candidate.source.describes_itself() < known.source.describes_itself() {
                known.source = candidate.source;
                known.kind = candidate.kind;
                known.detail = candidate.detail;
            }
            continue;
        }
        let Some(found) = score(folded, &needle, &candidate.name, &mut scratch) else {
            continue;
        };
        seen.insert(candidate.name.clone(), rows.len());
        rows.push(Row {
            name: candidate.name,
            source: candidate.source,
            kind: candidate.kind,
            detail: candidate.detail,
            matched: found.matched,
            score: found.score,
        });
    }
    // Sorted on a key worked out once a row rather than inside the comparison, which would have
    // counted every name's characters again at every one of its comparisons. Every part of it is an
    // integer or the name's own bytes, so the order is total and the same on every machine — which
    // is what the determinism property rests on.
    let mut order: Vec<(i32, u8, usize, usize)> = rows
        .iter()
        .enumerate()
        .map(|(at, row)| (-row.score, row.source.order(), row.name.chars().count(), at))
        .collect();
    order.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
            .then(rows[left.3].name.as_bytes().cmp(rows[right.3].name.as_bytes()))
    });
    let mut taken: Vec<Option<Row>> = rows.into_iter().map(Some).collect();
    order.into_iter().filter_map(|(_, _, _, at)| taken[at].take()).collect()
}

/// Every candidate as a row, deduplicated and in the tie-break's order. What an empty stem offers.
///
/// No scoring, because there is nothing to score against: every row's score is zero and no letter
/// of any name is marked. The deduplication is the same rule [`rank`] uses — two entries that would
/// type the same bytes are one offer, and the source that describes itself best keeps the label.
fn everything(candidates: Vec<Candidate>) -> Vec<Row> {
    let mut seen: std::collections::HashMap<String, usize> =
        std::collections::HashMap::with_capacity(candidates.len());
    let mut rows: Vec<Row> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if candidate.name.is_empty() {
            continue;
        }
        if let Some(at) = seen.get(&candidate.name) {
            let known: &mut Row = &mut rows[*at];
            if candidate.source.describes_itself() < known.source.describes_itself() {
                known.source = candidate.source;
                known.kind = candidate.kind;
                known.detail = candidate.detail;
            }
            continue;
        }
        seen.insert(candidate.name.clone(), rows.len());
        rows.push(Row {
            name: candidate.name,
            source: candidate.source,
            kind: candidate.kind,
            detail: candidate.detail,
            matched: Vec::new(),
            score: 0,
        });
    }
    rows.sort_by(|left, right| {
        left.source
            .order()
            .cmp(&right.source.order())
            .then(left.name.chars().count().cmp(&right.name.chars().count()))
            .then(left.name.as_bytes().cmp(right.name.as_bytes()))
    });
    rows
}

/// Working space reused across a whole pool, so scoring a project's worth of names allocates once.
///
/// The candidate's characters, their folded copies and the alignment table are all per-candidate
/// scratch, and building three vectors for each of a few thousand names was the largest single cost
/// of a keystroke when this was measured on Quill's own biggest file. Kept and cleared instead: the
/// answer is identical and nothing is allocated after the first candidate.
#[derive(Default)]
struct Scratch {
    letters: Vec<char>,
    lowered: Vec<char>,
    table: Vec<i32>,
}

/// What one candidate scored, and where the stem landed in it.
struct Scored {
    score: i32,
    matched: Vec<usize>,
}

/// Score one candidate against one stem, or nothing when the stem is not a subsequence of it.
///
/// `folded` is the stem's letters lowercased and `typed` is them as they were typed; both are
/// needed, because the match is case-insensitive and one of the bonuses is not.
fn score(folded: &[char], typed: &[char], name: &str, scratch: &mut Scratch) -> Option<Scored> {
    scratch.letters.clear();
    scratch.letters.extend(name.chars());
    scratch.lowered.clear();
    for at in 0..scratch.letters.len() {
        let folded_letter = lower(scratch.letters[at]);
        scratch.lowered.push(folded_letter);
    }
    if !is_subsequence(folded, &scratch.lowered) {
        return None;
    }
    let alignment = align(folded, typed, scratch)?;
    let mut score = alignment.score;
    // Starting with the stem is the commonest intent by far, and it is a property of the whole
    // candidate rather than of any one letter, so it is added once here.
    if scratch.lowered.len() >= folded.len() && scratch.lowered[..folded.len()] == *folded {
        score += PREFIX;
    }
    score -= UNMATCHED * (scratch.letters.len().saturating_sub(folded.len())) as i32;
    Some(Scored { score, matched: alignment.matched })
}

/// Whether `needle` appears in `haystack` in order, both already folded. The cheap reject: nearly
/// every candidate in a project fails here, and it costs one walk of two short strings.
fn is_subsequence(needle: &[char], haystack: &[char]) -> bool {
    let mut at = 0;
    for letter in haystack {
        if at < needle.len() && needle[at] == *letter {
            at += 1;
        }
    }
    at == needle.len()
}

/// The best alignment of a stem inside a name, and what it scored.
struct Alignment {
    score: i32,
    matched: Vec<usize>,
}

/// Find the **best** alignment rather than the first one.
///
/// `pt` lines up with `paint_text` two ways — the `t` of `paint` or the `t` of `text` — and only
/// the second sits on a word boundary, which is the one a person meant. Sublime finds this by
/// bounded recursion; the same answer comes out of filling a small table, which is what this does,
/// and a table cannot run out of recursion budget half way through a long name and silently return
/// the worse reading.
///
/// The table is `stem × name × whether the letter before was matched`, because the consecutive
/// bonus is the one thing a cell's value depends on outside itself. Identifiers are short: a three
/// letter stem in a twelve letter name is seventy-two cells.
fn align(folded: &[char], typed: &[char], scratch: &mut Scratch) -> Option<Alignment> {
    let letters = &scratch.letters;
    let lowered = &scratch.lowered;
    let stem = folded.len();
    let name = letters.len();
    // `best[(i * (name + 1) + j) * 2 + run]` is the best total for stem[i..] inside name[j..].
    let width = (name + 1) * 2;
    let best = &mut scratch.table;
    best.clear();
    best.resize((stem + 1) * width, IMPOSSIBLE);
    let cell = |i: usize, j: usize, run: bool| (i * width) + j * 2 + usize::from(run);
    for j in 0..=name {
        best[cell(stem, j, false)] = 0;
        best[cell(stem, j, true)] = 0;
    }
    for i in (0..stem).rev() {
        for j in (0..name).rev() {
            for run in [false, true] {
                // Step over this letter of the name. The run of consecutive matches ends here.
                let mut value = best[cell(i, j + 1, false)];
                if folded[i] == lowered[j] {
                    let mut bonus = 0;
                    if is_boundary(letters, j) {
                        bonus += BOUNDARY;
                    }
                    if run {
                        bonus += CONSECUTIVE;
                    }
                    if typed[i] == letters[j] {
                        bonus += SAME_CASE;
                    }
                    let rest = best[cell(i + 1, j + 1, true)];
                    if rest > IMPOSSIBLE {
                        value = value.max(bonus + rest);
                    }
                }
                best[cell(i, j, run)] = value;
            }
        }
    }
    let total = best[cell(0, 0, false)];
    if total <= IMPOSSIBLE {
        return None;
    }
    // Walk the table back out to say which letters the best reading used. Taking the match whenever
    // it is as good as stepping over settles a tie towards the earlier letter, which is what makes
    // the picked-out letters the same on every run.
    let mut matched = Vec::with_capacity(stem);
    let (mut i, mut j, mut run) = (0, 0, false);
    while i < stem && j < name {
        let step = best[cell(i, j + 1, false)];
        let mut took = false;
        if folded[i] == lowered[j] {
            let mut bonus = 0;
            if is_boundary(letters, j) {
                bonus += BOUNDARY;
            }
            if run {
                bonus += CONSECUTIVE;
            }
            if typed[i] == letters[j] {
                bonus += SAME_CASE;
            }
            let rest = best[cell(i + 1, j + 1, true)];
            took = rest > IMPOSSIBLE && bonus + rest >= step;
        }
        if took {
            matched.push(j);
            i += 1;
            j += 1;
            run = true;
        } else {
            j += 1;
            run = false;
        }
    }
    Some(Alignment { score: total, matched })
}

/// Whether the letter at `at` starts the name or a part of it.
///
/// The start, anything after a character that is not a letter or a digit — `_`, `-`, `@`, `$`, the
/// three separators the languages Quill reads actually use — and a lower-to-upper camel step. Not
/// asked of the grammar, deliberately: a hyphen is *inside* a CSS word, which is exactly why it is
/// a boundary within one.
fn is_boundary(letters: &[char], at: usize) -> bool {
    if at == 0 {
        return true;
    }
    let before = letters[at - 1];
    if !before.is_alphanumeric() {
        return true;
    }
    before.is_lowercase() && letters[at].is_uppercase()
}

/// One character folded for comparison. `char::to_lowercase` gives an iterator because a few
/// characters fold to more than one, and a subsequence match needs one character to stay one
/// character, so the first is taken and the rest — which no identifier in practice reaches — are
/// left alone.
fn lower(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rust as the bundled plugin describes it, cut down to what these tests need.
    fn rust() -> Grammar {
        let words = |list: &str| list.split(' ').map(str::to_owned).collect::<Vec<String>>();
        Grammar {
            language: "Rust".to_owned(),
            keywords: words("fn let mut const struct impl pub use match return"),
            builtins: words("String Vec Option Some None usize"),
            line_comment: Some("//".to_owned()),
            strings: vec!['"'],
            escapes: true,
            operators: "+-*/%=<>!&|^?:;,.#".chars().collect(),
            numbers: true,
            ..Grammar::default()
        }
    }

    #[test]
    fn an_empty_stem_offers_nothing_to_rank_and_everything_to_rank_all() {
        // `task-1680` §5.2. The guard is right for a word being typed and wrong inside an import,
        // where `from '|'` is a position at which the language itself says what comes next.
        let pool = || {
            vec![
                Candidate::described("./layout", Source::Module, Some(SymbolKind::Module), "src"),
                Candidate::described("./caret", Source::Module, Some(SymbolKind::Module), "src"),
                Candidate::new("draw", Source::Word),
            ]
        };
        assert!(rank("", pool()).is_empty(), "nothing is being completed");
        let all: Vec<String> = rank_all("", pool()).into_iter().map(|row| row.name).collect();
        assert_eq!(
            all,
            vec!["./caret".to_owned(), "./layout".to_owned(), "draw".to_owned()],
            "by source, then by the shorter name: a module comes before a word"
        );
    }

    #[test]
    fn a_module_wins_a_tie_with_a_name_spelt_the_same() {
        // The one place `Source::Module`'s order is read: `use a::b` with `b` both a module and a
        // function far more often means the module.
        let rows = rank(
            "part",
            vec![
                Candidate::described("parts", Source::Index, Some(SymbolKind::Function), "a.rs"),
                Candidate::described("parts", Source::Module, Some(SymbolKind::Module), "a/"),
            ],
        );
        assert_eq!(rows.len(), 1, "two entries that would type the same bytes are one offer");
        assert_eq!(rows[0].source, Source::Module);
        assert_eq!(rows[0].detail, "a/");
    }

    /// CSS, where a hyphen is a letter.
    fn css() -> Grammar {
        Grammar {
            language: "CSS".to_owned(),
            keywords: vec!["@media".to_owned()],
            builtins: vec!["background-color".to_owned()],
            operators: "{}();:,".chars().collect(),
            numbers: true,
            word_characters: vec!['-', '@'],
            ..Grammar::default()
        }
    }

    /// The names a stem offers, in order, out of a list of plain words.
    fn offered(stem: &str, names: &[&str]) -> Vec<String> {
        let pool = names.iter().map(|name| Candidate::new(*name, Source::Word)).collect();
        rank(stem, pool).into_iter().map(|row| row.name).collect()
    }

    #[test]
    fn a_prefix_wins_and_the_shorter_of_two_prefixes_wins_before_a_middle_match() {
        // Scenario 1.
        assert_eq!(offered("dra", &["draw_frame", "redraw", "draw"]), ["draw", "draw_frame", "redraw"]);
    }

    #[test]
    fn a_letter_on_a_word_boundary_is_worth_more_than_one_in_the_middle() {
        // Scenario 2: `pt` prefers `paint_text`, which needs the **best** alignment rather than the
        // first — the `t` of `paint` lines up too, and it is not on a boundary.
        assert_eq!(offered("pt", &["pointer", "paint_text"]), ["paint_text", "pointer"]);
        // The same at a camel step, which is the other kind of boundary.
        assert_eq!(offered("pt", &["pointer", "paintText"]), ["paintText", "pointer"]);
    }

    #[test]
    fn the_match_is_case_insensitive_and_the_case_bonus_never_excludes() {
        // Scenario 3.
        assert_eq!(offered("LYT", &["layout"]), ["layout"]);
        assert_eq!(offered("lyt", &["layout"]), ["layout"]);
        // Typed as it is spelled, the same name still wins against one spelled differently.
        assert_eq!(offered("lay", &["Layout", "layout"]), ["layout", "Layout"]);
    }

    #[test]
    fn the_row_equal_to_the_stem_is_never_offered() {
        // Scenario 4. It is what makes `Enter` mean a new line once a word is completely typed.
        assert_eq!(offered("draw", &["draw", "draw_frame", "redraw"]), ["draw_frame", "redraw"]);
        assert!(offered("draw", &["draw"]).is_empty());
    }

    #[test]
    fn a_stem_that_matches_nothing_offers_nothing_rather_than_everything() {
        // Scenario 5.
        assert!(offered("zzz", &["draw", "layout", "paint_text"]).is_empty());
        assert!(offered("", &["draw"]).is_empty(), "and an empty stem asks nothing at all");
    }

    #[test]
    fn two_sources_offering_one_spelling_are_one_row_labelled_from_the_better_one() {
        // Scenario 6. A definition beats a keyword beats a plain word.
        let pool = vec![
            Candidate::new("let", Source::Word),
            Candidate::described("let", Source::Language, None, "keyword"),
        ];
        let rows = rank("le", pool);
        assert_eq!(rows.len(), 1, "one row, because both would type the same bytes: {rows:?}");
        assert_eq!(rows[0].source, Source::Language);
        assert_eq!(rows[0].detail, "keyword");

        let pool = vec![
            Candidate::described("draw", Source::Language, None, "keyword"),
            Candidate::described("draw", Source::ThisFile, Some(SymbolKind::Function), "layout.rs"),
            Candidate::new("draw", Source::Word),
        ];
        let rows = rank("dr", pool);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, Source::ThisFile, "a definition has the most to say");
        assert_eq!(rows[0].kind, Some(SymbolKind::Function));
    }

    #[test]
    fn equally_scored_rows_are_offered_nearest_source_first() {
        // Scenario 7. One name each, so the scores are as close as they can be made and only the
        // source decides.
        let pool = vec![
            Candidate::described("drawc", Source::Language, None, "keyword"),
            Candidate::described("drawb", Source::Index, Some(SymbolKind::Function), "far.rs"),
            Candidate::new("drawd", Source::Word),
            Candidate::described("drawa", Source::ThisFile, Some(SymbolKind::Function), "here.rs"),
        ];
        let order: Vec<Source> = rank("draw", pool).into_iter().map(|row| row.source).collect();
        assert_eq!(
            order,
            [Source::ThisFile, Source::Word, Source::Index, Source::Language],
            "this file's definitions, then its words, then the project's, then the language's"
        );
    }

    #[test]
    fn letters_wider_than_one_byte_match_and_land_on_character_positions() {
        // Scenario 8. The positions are what picks letters out of a drawn name, and a byte position
        // would be a different number after an accent.
        let rows = rank("dj", vec![Candidate::new("d\u{00E9}j\u{00E0}", Source::Word)]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].matched, vec![0, 2], "the d and the j, counted in characters");
        // And a script with no case at all is matched the same way.
        let rows = rank("\u{6771}", vec![Candidate::new("\u{6771}\u{4EAC}", Source::Word)]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].matched, vec![0]);
    }

    #[test]
    fn a_css_custom_property_is_one_word_and_its_hyphens_are_boundaries() {
        // Scenario 9.
        let text = "--brand-hue: 280;\n.card { color: var(--br";
        let stem = stem_at(text, text.len(), &css());
        assert_eq!(&text[stem.clone()], "--br", "the hyphens are word characters here");
        let pool = vec![
            Candidate::new("--brand-hue", Source::Word),
            Candidate::new("border-radius", Source::Word),
        ];
        let names: Vec<String> =
            rank(&text[stem], pool).into_iter().map(|row| row.name).collect();
        assert_eq!(names, ["--brand-hue"], "`border-radius` has no `--` in it at all");
    }

    #[test]
    fn scoring_the_same_stem_against_the_same_pool_twice_gives_the_same_list() {
        // Scenario 10, and the determinism property.
        let pool = || {
            vec![
                Candidate::new("draw", Source::Word),
                Candidate::new("draw_frame", Source::Word),
                Candidate::described("draw_all", Source::Index, Some(SymbolKind::Function), "a.rs"),
                Candidate::described("redraw", Source::OpenTab, Some(SymbolKind::Function), "b.rs"),
                Candidate::described("drop", Source::Language, None, "keyword"),
            ]
        };
        assert_eq!(rank("dr", pool()), rank("dr", pool()));
        assert_eq!(rank("dra", pool()), rank("dra", pool()));
    }

    #[test]
    fn the_stem_is_what_has_been_typed_of_the_word_and_nothing_to_the_right_of_it() {
        let text = "let value = dra";
        assert_eq!(&text[stem_at(text, text.len(), &rust())], "dra");
        // Mid-word: the stem is the left half and the word is the whole of it.
        let text = "drawing";
        assert_eq!(&text[stem_at(text, 3, &rust())], "dra");
        assert_eq!(&text[word_at(text, 3, &rust())], "drawing");
        // With nothing to the right, the two are the same thing.
        assert_eq!(stem_at(text, 7, &rust()), word_at(text, 7, &rust()));
    }

    #[test]
    fn a_point_that_is_not_on_a_word_has_no_stem_at_all() {
        // Which is what `Ctrl+Space` reports as an honest miss rather than opening an empty list.
        let text = "let value = 42;";
        assert!(stem_at(text, 0, &rust()).is_empty(), "the very start of the file");
        assert!(stem_at(text, 4, &rust()).is_empty(), "just after a space");
        assert!(stem_at(text, 14, &rust()).is_empty(), "just after a number");
        assert!(stem_at(text, text.len(), &rust()).is_empty(), "just after a semicolon");
        assert!(stem_at("", 0, &rust()).is_empty());
        // A digit cannot start a word, so `x2` is a stem and the `2` of `42` is not.
        assert_eq!(&"let x2"[stem_at("let x2", 6, &rust())], "x2");
    }

    #[test]
    fn an_offset_that_is_not_a_character_boundary_answers_nothing_rather_than_panicking() {
        let text = "d\u{00E9}j\u{00E0}";
        assert!(stem_at(text, 2, &rust()).is_empty(), "inside the é");
        assert!(stem_at(text, 99, &rust()).is_empty(), "past the end");
        assert!(word_at(text, 99, &rust()).is_empty());
    }

    #[test]
    fn the_cheap_reject_agrees_with_the_scorer_about_what_matches() {
        // The window uses it to decide which of four thousand names are worth building a candidate
        // out of, so a name it lets through that the scorer then drops is waste, and one it drops
        // that the scorer would have offered is a missing row.
        let names = [
            "draw", "draw_frame", "redraw", "layout", "paint_text", "--brand-hue", "d\u{00E9}j\u{00E0}", "x",
        ];
        for stem in ["d", "dr", "dra", "lyt", "pt", "--br", "zz", "drawn", "\u{00E9}"] {
            for name in names {
                let offered = !rank(stem, vec![Candidate::new(name, Source::Word)]).is_empty();
                let cheap = could_match(stem, name) && name != stem;
                assert_eq!(offered, cheap, "{stem} against {name}");
            }
        }
        assert!(!could_match("", "draw"), "an empty stem asks nothing");
    }

    #[test]
    fn a_row_says_which_of_its_letters_were_matched() {
        let rows = rank("ptx", vec![Candidate::new("paint_text", Source::Word)]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let name: Vec<char> = "paint_text".chars().collect();
        let picked: String = rows[0].matched.iter().map(|at| name[*at]).collect();
        assert_eq!(picked, "ptx", "the picked out letters spell what was typed");
        assert_eq!(rows[0].matched, vec![0, 6, 8], "on the boundaries, which is the best reading");
    }

    /// **Truthfulness**: every row really is a case-insensitive subsequence match, its matched
    /// positions are inside the name, in order, and spell the stem back.
    #[test]
    fn every_row_offered_is_one_the_stem_really_matches() {
        for stem in ["d", "dr", "dra", "LYT", "pt", "--br", "\u{00E9}"] {
            for row in rank(stem, a_pool()) {
                let letters: Vec<char> = row.name.chars().collect();
                assert_eq!(row.matched.len(), stem.chars().count(), "{stem} against {}", row.name);
                let mut last = None;
                for at in &row.matched {
                    assert!(*at < letters.len(), "{at} is outside {}", row.name);
                    assert!(last.is_none_or(|before| before < *at), "in order");
                    last = Some(*at);
                }
                let picked: String =
                    row.matched.iter().map(|at| lower(letters[*at])).collect();
                let wanted: String = stem.chars().map(lower).collect();
                assert_eq!(picked, wanted, "{stem} against {}", row.name);
                assert_ne!(row.name, stem, "the row equal to the stem is never offered");
            }
        }
    }

    /// **Determinism**: same stem, same pool, same list — including the picked out letters.
    #[test]
    fn the_same_question_always_gets_the_same_answer() {
        for stem in ["d", "dr", "dra", "aw", "e", "n"] {
            assert_eq!(rank(stem, a_pool()), rank(stem, a_pool()), "{stem}");
        }
    }

    /// **Isolation**: nothing here reads or writes anything. There is no filesystem call to test
    /// for, so what is exercised is that every entry point answers from a `&str`, a `Grammar` and a
    /// list of names, with no path in sight.
    #[test]
    fn every_answer_comes_from_the_text_the_grammar_and_the_pool_alone() {
        let text = "fn draw() { let dra";
        let grammar = rust();
        let stem = stem_at(text, text.len(), &grammar);
        assert_eq!(&text[stem.clone()], "dra");
        assert_eq!(word_at(text, text.len(), &grammar), stem);
        assert!(!rank(&text[stem], a_pool()).is_empty());
    }

    /// Every shape of candidate worth holding the properties against.
    fn a_pool() -> Vec<Candidate> {
        vec![
            Candidate::new("draw", Source::Word),
            Candidate::new("draw_frame", Source::Word),
            Candidate::new("redraw", Source::Word),
            Candidate::new("d", Source::Word),
            Candidate::new("d\u{00E9}j\u{00E0}", Source::Word),
            Candidate::new("--brand-hue", Source::Word),
            Candidate::new("paintText", Source::Word),
            Candidate::described("layout", Source::ThisFile, Some(SymbolKind::Type), "layout.rs"),
            Candidate::described("new", Source::Index, Some(SymbolKind::Function), "caret.rs"),
            Candidate::described("let", Source::Language, None, "keyword"),
            Candidate::new(String::new(), Source::Word),
        ]
    }
}
