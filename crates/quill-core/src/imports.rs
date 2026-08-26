//! What the caret is in the middle of importing.
//!
//! [`crate::completion`] answers "which names could this word become"; this module answers the
//! question asked one step earlier and only inside an import statement: *is this a position where
//! the language itself says what comes next, and if so what has been written of it so far*. Like
//! the two modules beside it, it is pure — a `&str`, an offset and a [`Grammar`] in, a [`Context`]
//! out. It reads no disk, draws nothing, and its tests run with no window.
//!
//! It does **not** decide what the answer is. Resolving `'./layout'` to a real file and turning a
//! real file back into a specifier both need the project, so both live in
//! `quill_app::services::imports`. What is here is only the reading of the text.
//!
//! ## Two families, because there are two shapes and no third
//!
//! ```text
//! import { Layout } from './layout';     the module is a string
//! @import 'theme.css';                   the module is a string
//! use quill_core::completion::Row;       the module is a path of segments
//! ```
//!
//! A string is resolved against the file system, relative to the file being edited. A path of
//! segments is resolved against a module tree, which is the file system read through the language's
//! own rules about where a module lives. They share nothing but the popup they end up in, so they
//! are two readings and one enum. `language.imports` says which a language has, and a language that
//! named neither never reaches this module at all.
//!
//! ## Both readings work backwards from the caret
//!
//! For the same reason [`crate::symbols::FileSymbols::identifier_at`] works from the point rather
//! than from a parse: it costs a few hundred bytes of scanning per keystroke instead of a reading
//! of the file, and a file whose earlier lines are half-typed cannot poison the answer.
//!
//! The path family's walk **is** its parse, and the keyword it ends at is what makes it
//! trustworthy: `use` in front and it is an import, anything else in front and `a::b::c` is
//! ordinary code that must be offered the ordinary four sources. There is no separate search for
//! the keyword and no line budget on it, because the walk either reaches it or it does not.
//!
//! `tasks/task-1680-import-completion-tdd.md` §4 is the design.

use std::ops::Range;

use crate::completion;
use crate::syntax::{Grammar, ImportStyle};

/// How far above the caret a statement may begin.
///
/// A named import list wrapped by a formatter is a handful of lines; twenty-four is far past any
/// of them and is still small enough that the walk cannot become a reading of the file. What it
/// really guards is the case where none of the shapes below is present at all: a budget is what
/// stops "is there an import keyword somewhere above me" from walking to the top of a large file
/// on every keystroke.
const STATEMENT_LINES: usize = 24;

/// How many segments a module path may have before the walk gives up.
const MAX_SEGMENTS: usize = 32;

/// What the caret is in the middle of importing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Context {
    /// Inside the module specifier of an import: `from './lay│'`.
    Specifier {
        /// What has been typed of it: the string's content left of the caret. What `Enter`
        /// replaces.
        typed: Range<usize>,
        /// The whole of the string's content, never including its quotes. What `Tab` replaces.
        ///
        /// It is here rather than worked out from the grammar because
        /// [`completion::word_at`] cannot answer it: a specifier is not made of word characters,
        /// and the grammar would read `./lay/out` as three words.
        whole: Range<usize>,
    },
    /// Inside the braces of an import whose module is written: `import { Lay│ } from './layout'`.
    Named {
        /// The module as it is written, which the window resolves against the project.
        module: String,
        /// The word being typed, exactly as [`completion::stem_at`] reads one anywhere else.
        stem: Range<usize>,
    },
    /// Inside a path-family import: `use quill_core::comp│`.
    Segment {
        /// The segments already written, outermost first. Empty at `use │`, which is the position
        /// that offers the reserved roots and the packages.
        segments: Vec<String>,
        stem: Range<usize>,
    },
}

impl Context {
    /// The range a completion replaces when the whole of it is being replaced — `Tab`.
    ///
    /// A specifier answers with its own string, because the grammar cannot; the other two are
    /// ordinary identifiers and say so by answering nothing, which leaves the caller on
    /// [`completion::word_at`] exactly as every other row is.
    pub fn whole_range(&self) -> Option<Range<usize>> {
        match self {
            Context::Specifier { whole, .. } => Some(whole.clone()),
            _ => None,
        }
    }

    /// The range a completion replaces when only what was typed is being replaced — `Enter`.
    pub fn typed_range(&self) -> Range<usize> {
        match self {
            Context::Specifier { typed, .. } => typed.clone(),
            Context::Named { stem, .. } => stem.clone(),
            Context::Segment { stem, .. } => stem.clone(),
        }
    }

    /// True for the one context that lives inside a string, where the popup's usual refusal to open
    /// in a comment or a string has to be asked of the import instead.
    pub fn is_specifier(&self) -> bool {
        matches!(self, Context::Specifier { .. })
    }
}

/// What the caret is in the middle of importing, or nothing.
///
/// Nothing is by far the commonest answer, and it is reached in one comparison for a language that
/// named no imports at all.
pub fn context_at(text: &str, offset: usize, grammar: &Grammar) -> Option<Context> {
    if !grammar.completes_imports() || offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    match grammar.imports? {
        ImportStyle::Quoted => quoted(text, offset, grammar),
        ImportStyle::Path => segments(text, offset, grammar),
    }
}

/// The quoted family: three questions, and the first one to answer wins.
fn quoted(text: &str, offset: usize, grammar: &Grammar) -> Option<Context> {
    // 1. Inside a string on this line? Then it is the specifier, if the statement is an import.
    if let Some(whole) = string_around(text, offset, grammar) {
        let quote = whole.start.saturating_sub(1);
        if commented_out(text, quote, grammar) || !keyword_in_statement(text, quote, grammar) {
            return None;
        }
        return Some(Context::Specifier { typed: whole.start..offset, whole });
    }
    // 2. Inside an unclosed `{`? Then it is a named import list, if an import keyword is in front
    //    of the brace. Only whitespace, words and commas may be between the two, which is what
    //    keeps `function draw() {` and `class Layout {` from reading as import lists.
    let brace = open_brace_before(text, offset)?;
    keyword_before(text, brace, grammar)?;
    // 3. The module is written after the caret, so it is read forwards from it.
    let module = module_after(text, offset, grammar)?;
    let stem = completion::stem_at(text, offset, grammar);
    Some(Context::Named { module, stem })
}

/// The path family: one backwards walk, ending at the keyword or at nothing.
fn segments(text: &str, offset: usize, grammar: &Grammar) -> Option<Context> {
    let separator = grammar.path_separator.as_deref().filter(|it| !it.is_empty())?;
    let stem = completion::stem_at(text, offset, grammar);
    let mut written: Vec<String> = Vec::new();
    let mut at = stem.start;
    for _ in 0..MAX_SEGMENTS {
        at = space_back(text, at)?;
        if text[..at].ends_with(separator) {
            at = space_back(text, at - separator.len())?;
            let word = word_back(text, at, grammar);
            if word.is_empty() {
                return None;
            }
            written.push(text[word.clone()].to_owned());
            at = word.start;
            continue;
        }
        // A brace is stepped over: `a::{b│` carries on with the outer path.
        if text[..at].ends_with('{') {
            at -= 1;
            continue;
        }
        // A comma means the caret is in a sibling list, so the whole list is stepped back over to
        // the brace that opened it: `a::{b, c│}` is still `a`.
        if text[..at].ends_with(',') {
            at = before_the_list(text, at)?;
            continue;
        }
        break;
    }
    // The anchor. Without the keyword in front of it this is `a::b::c` in ordinary code, and the
    // ordinary four sources are what it should be offered.
    let at = space_back(text, at)?;
    let word = word_back(text, at, grammar);
    if word.is_empty() || !grammar.import_keywords.iter().any(|it| *it == text[word.clone()]) {
        return None;
    }
    written.reverse();
    Some(Context::Segment { segments: written, stem })
}

/// The content of the string the caret is inside, without its quotes.
///
/// The line the caret is on and no more, because a module specifier does not span lines in any of
/// the three languages that write one. A string left unterminated — which is what one being typed
/// always is — runs to the end of the line, so `from './lay│` answers before the closing quote has
/// been typed.
fn string_around(text: &str, offset: usize, grammar: &Grammar) -> Option<Range<usize>> {
    if grammar.strings.is_empty() {
        return None;
    }
    let start = text[..offset].rfind('\n').map(|at| at + 1).unwrap_or(0);
    let end = text[offset..].find('\n').map(|at| offset + at).unwrap_or(text.len());
    let line = &text[start..end];
    let mut open: Option<(char, usize)> = None;
    let mut at = 0;
    while at < line.len() {
        let letter = line[at..].chars().next().expect("inside the line");
        let width = letter.len_utf8();
        match open {
            Some((quote, from)) => {
                if grammar.escapes && letter == '\\' {
                    at += width;
                    at += line[at..].chars().next().map_or(0, char::len_utf8);
                    continue;
                }
                if letter == quote {
                    let content = (start + from)..(start + at);
                    if content.contains(&offset) || content.end == offset {
                        return Some(content);
                    }
                    open = None;
                }
            }
            None => {
                if grammar.strings.contains(&letter) {
                    open = Some((letter, at + width));
                }
            }
        }
        at += width;
    }
    // An unterminated string, which is what one being typed is: it runs to the end of the line.
    let (_, from) = open?;
    let content = (start + from)..end;
    (content.contains(&offset) || content.end == offset).then_some(content)
}

/// Whether a line comment opens on this line before `at`.
///
/// A specifier is the one context that lives inside a string, so the popup's usual refusal to open
/// inside a comment cannot be what keeps a list from appearing over `// import x from './a'`.
fn commented_out(text: &str, at: usize, grammar: &Grammar) -> bool {
    let Some(marker) = grammar.line_comment.as_deref().filter(|it| !it.is_empty()) else {
        return false;
    };
    let start = text[..at].rfind('\n').map(|found| found + 1).unwrap_or(0);
    text[start..at].contains(marker)
}

/// Whether the statement holding `at` begins with one of the language's import keywords.
///
/// The specifier's anchor, and it cannot be the walk [`keyword_before`] does: the path from a
/// specifier back to its keyword crosses the named list — `import { A } from './x'` — and `}` is
/// not a word, a comma or a space. So the statement is bounded first and then searched.
///
/// The bound is what makes this honest. It is not "somewhere in the twenty-four lines above": it
/// is *this statement*, which ends at a `;` or at a line break, with a bracketed group stepped over
/// whole so a named list written across four lines is still one statement. Without that,
/// `import x from 'y'` on the line above would make the next line's ordinary string an import.
fn keyword_in_statement(text: &str, at: usize, grammar: &Grammar) -> bool {
    let Some(start) = statement_start(text, at) else {
        return false;
    };
    let window = &text[start..at];
    grammar.import_keywords.iter().any(|keyword| whole_word_in(window, keyword, grammar))
}

/// Whether `keyword` appears in `window` with a non-word character either side of it.
fn whole_word_in(window: &str, keyword: &str, grammar: &Grammar) -> bool {
    if keyword.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(found) = window[from..].find(keyword) {
        let begin = from + found;
        let end = begin + keyword.len();
        let before = window[..begin].chars().next_back();
        let after = window[end..].chars().next();
        let clear = |letter: Option<char>| {
            letter.is_none_or(|letter| !grammar.is_word_character(letter, false))
        };
        if clear(before) && clear(after) {
            return true;
        }
        from = begin + keyword.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Where the statement holding `at` begins: after the last `;` or line break outside a bracketed
/// group, or at the top of the file.
fn statement_start(text: &str, at: usize) -> Option<usize> {
    let mut cursor = at;
    for _ in 0..MAX_SEGMENTS {
        while cursor > 0 {
            let letter = text[..cursor].chars().next_back().expect("inside the text");
            match letter {
                ';' | '\n' => return Some(cursor),
                '}' | ')' | ']' => break,
                _ => cursor -= letter.len_utf8(),
            }
        }
        if cursor == 0 {
            return Some(0);
        }
        cursor = matching_open(text, cursor)?;
    }
    None
}

/// The position of the bracket that opened the group ending just before `cursor`.
fn matching_open(text: &str, cursor: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut lines = 0usize;
    for (at, letter) in text[..cursor].char_indices().rev() {
        match letter {
            '\n' => {
                lines += 1;
                if lines > STATEMENT_LINES {
                    return None;
                }
            }
            '}' | ')' | ']' => depth += 1,
            '{' | '(' | '[' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The end of the import keyword in front of `at`, or nothing.
///
/// Only whitespace, words and commas may be between the two. That is a strong filter and it is what
/// does most of the work here: `function draw() {` and `if (ready) {` both have a bracket in front
/// of the brace and so are refused before any word is read, while `import type {` and
/// `import { A } from '…'` both reach their keyword over words alone.
fn keyword_before(text: &str, at: usize, grammar: &Grammar) -> Option<usize> {
    let mut at = at;
    for _ in 0..MAX_SEGMENTS {
        at = space_back(text, at)?;
        if text[..at].ends_with(',') {
            at -= 1;
            continue;
        }
        let word = word_back(text, at, grammar);
        if word.is_empty() {
            return None;
        }
        if grammar.import_keywords.iter().any(|it| *it == text[word.clone()]) {
            return Some(word.end);
        }
        at = word.start;
    }
    None
}

/// The innermost `{` left of the caret that has not been closed, or nothing.
///
/// Bounded by a `;` and by [`STATEMENT_LINES`], so this is a walk of a statement rather than of the
/// file.
fn open_brace_before(text: &str, offset: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut lines = 0usize;
    for (at, letter) in text[..offset].char_indices().rev() {
        match letter {
            '\n' => {
                lines += 1;
                if lines > STATEMENT_LINES {
                    return None;
                }
            }
            ';' => return None,
            '}' => depth += 1,
            '{' => match depth {
                0 => return Some(at),
                _ => depth -= 1,
            },
            _ => {}
        }
    }
    None
}

/// The position just after the `{` that opened the list the caret is in a later item of.
fn before_the_list(text: &str, at: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut lines = 0usize;
    for (found, letter) in text[..at].char_indices().rev() {
        match letter {
            '\n' => {
                lines += 1;
                if lines > STATEMENT_LINES {
                    return None;
                }
            }
            ';' => return None,
            '}' => depth += 1,
            '{' => match depth {
                0 => return Some(found + 1),
                _ => depth -= 1,
            },
            _ => {}
        }
    }
    None
}

/// The module a named-import list belongs to, read forwards from the caret.
///
/// It is written after the list — `import { Lay│ } from './layout'` — so it cannot be found by the
/// backwards walk. The scan stops the moment the list closes and the line ends, which is what keeps
/// a half-typed `import { Lay│ }` from taking the *next* statement's module and answering with it.
fn module_after(text: &str, offset: usize, grammar: &Grammar) -> Option<String> {
    let mut depth = 1usize;
    let mut lines = 0usize;
    let rest = &text[offset..];
    let mut at = 0usize;
    while at < rest.len() {
        let letter = rest[at..].chars().next().expect("inside the text");
        let width = letter.len_utf8();
        match letter {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ';' => return None,
            '\n' => {
                lines += 1;
                if depth == 0 || lines > STATEMENT_LINES {
                    return None;
                }
            }
            _ if depth == 0 && grammar.strings.contains(&letter) => {
                let content = string_around(text, offset + at + width, grammar)?;
                return Some(text[content].to_owned());
            }
            _ => {}
        }
        at += width;
    }
    None
}

/// Step back over the whitespace before `at`, giving up after [`STATEMENT_LINES`] line breaks.
fn space_back(text: &str, at: usize) -> Option<usize> {
    let mut at = at;
    let mut lines = 0usize;
    while let Some(letter) = text[..at].chars().next_back() {
        if !letter.is_whitespace() {
            break;
        }
        if letter == '\n' {
            lines += 1;
            if lines > STATEMENT_LINES {
                return None;
            }
        }
        at -= letter.len_utf8();
    }
    Some(at)
}

/// The word ending exactly at `at`, or an empty range when there is not one.
///
/// The grammar's own word characters, and the grammar's own rule about which of them a word may
/// start with — the same two questions [`completion::stem_at`] asks, so `@import` is one word in
/// CSS and `42` is not a word anywhere.
fn word_back(text: &str, at: usize, grammar: &Grammar) -> Range<usize> {
    let mut start = at;
    for (index, letter) in text[..at].char_indices().rev() {
        if !grammar.is_word_character(letter, false) {
            break;
        }
        start = index;
    }
    while start < at {
        let letter = text[start..].chars().next().expect("inside the word");
        if grammar.is_word_character(letter, true) {
            break;
        }
        start += letter.len_utf8();
    }
    start..at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::PathRoot;

    /// A grammar shaped like the TypeScript plugin's, cut down to what this module reads.
    fn typescript() -> Grammar {
        Grammar {
            keywords: ["import", "export", "from", "type", "const", "function", "require"]
                .iter()
                .map(|word| (*word).to_owned())
                .collect(),
            line_comment: Some("//".to_owned()),
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"', '\'', '`'],
            escapes: true,
            imports: Some(ImportStyle::Quoted),
            import_keywords: ["import", "export", "require"]
                .iter()
                .map(|word| (*word).to_owned())
                .collect(),
            import_extensions: [".ts", ".tsx", ".js"].iter().map(|it| (*it).to_owned()).collect(),
            import_index: vec!["index".to_owned()],
            import_omit_extension: true,
            export_keyword: Some("export".to_owned()),
            ..Grammar::default()
        }
    }

    /// A grammar shaped like the CSS plugin's: a hyphen and an at sign are letters.
    fn css() -> Grammar {
        Grammar {
            strings: vec!['"', '\''],
            escapes: true,
            word_characters: vec!['-', '@'],
            imports: Some(ImportStyle::Quoted),
            import_keywords: vec!["@import".to_owned()],
            import_extensions: vec![".css".to_owned()],
            import_omit_extension: false,
            ..Grammar::default()
        }
    }

    /// A grammar shaped like the Rust plugin's.
    fn rust() -> Grammar {
        Grammar {
            keywords: ["use", "pub", "let", "as", "mod"].iter().map(|it| (*it).to_owned()).collect(),
            line_comment: Some("//".to_owned()),
            strings: vec!['"'],
            escapes: true,
            imports: Some(ImportStyle::Path),
            import_keywords: vec!["use".to_owned()],
            import_extensions: vec![".rs".to_owned()],
            import_index: ["mod", "lib", "main"].iter().map(|it| (*it).to_owned()).collect(),
            export_keyword: Some("pub".to_owned()),
            path_separator: Some("::".to_owned()),
            source_roots: vec!["src".to_owned()],
            path_roots: vec![
                ("crate".to_owned(), PathRoot::Package),
                ("self".to_owned(), PathRoot::Module),
                ("super".to_owned(), PathRoot::Parent),
            ],
            ..Grammar::default()
        }
    }

    /// Read the context at the `|` in `text`, which is taken out before anything is asked.
    fn at(text: &str, grammar: &Grammar) -> Option<Context> {
        let offset = text.find('|').expect("the sample marks the caret with |");
        let text = text.replace('|', "");
        context_at(&text, offset, grammar)
    }

    /// What was typed of a specifier, for a test that only cares about that.
    fn typed(text: &str, grammar: &Grammar) -> Option<String> {
        let offset = text.find('|').expect("the sample marks the caret with |");
        let cleaned = text.replace('|', "");
        match context_at(&cleaned, offset, grammar)? {
            Context::Specifier { typed, .. } => Some(cleaned[typed].to_owned()),
            other => panic!("expected a specifier, got {other:?}"),
        }
    }

    #[test]
    fn a_partly_typed_specifier_is_read_with_its_quotes_left_out() {
        // Scenario 1.
        let sample = "import { A } from './la|'";
        assert_eq!(typed(sample, &typescript()).as_deref(), Some("./la"));
        let context = at(sample, &typescript()).expect("a specifier");
        let text = sample.replace('|', "");
        let whole = context.whole_range().expect("a specifier answers with its own range");
        assert_eq!(&text[whole], "./la", "the quotes are never part of it");
    }

    #[test]
    fn a_specifier_with_nothing_typed_is_still_a_specifier() {
        // Scenario 2: this is the position the empty stem exists for.
        assert_eq!(typed("import { A } from '|'", &typescript()).as_deref(), Some(""));
    }

    #[test]
    fn the_three_other_shapes_of_a_quoted_import_read_the_same() {
        // Scenarios 3, 4, 5 and 6.
        let grammar = typescript();
        assert_eq!(typed("import A from \"./la|\"", &grammar).as_deref(), Some("./la"));
        assert_eq!(typed("import * as ns from './la|'", &grammar).as_deref(), Some("./la"));
        assert_eq!(typed("export { A } from './la|'", &grammar).as_deref(), Some("./la"));
        assert_eq!(typed("const x = require('./la|')", &grammar).as_deref(), Some("./la"));
    }

    #[test]
    fn css_reads_its_at_rule_as_the_keyword() {
        // Scenarios 7 and 8. `@import` is one word only because `@` is one of CSS's own word
        // characters, which is `task-1671`'s key doing a second job.
        assert_eq!(typed("@import 'the|'", &css()).as_deref(), Some("the"));
        assert_eq!(typed("@import url(\"the|\")", &css()).as_deref(), Some("the"));
    }

    #[test]
    fn a_string_with_no_import_keyword_in_front_of_it_is_not_a_specifier() {
        // Scenarios 9 and 10: an ordinary string, and a string in a second statement.
        let grammar = typescript();
        assert_eq!(at("const path = './la|'", &grammar), None);
        assert_eq!(at("import { A } from './a'; const p = './b|'", &grammar), None);
    }

    #[test]
    fn a_line_comment_holding_an_import_is_not_one() {
        assert_eq!(at("// import { A } from './la|'", &typescript()), None);
    }

    #[test]
    fn a_name_being_typed_between_the_braces_carries_the_module_it_is_about() {
        // Scenarios 11, 12 and 13.
        let grammar = typescript();
        let expected = Some(Context::Named { module: "./layout".to_owned(), stem: 9..11 });
        assert_eq!(at("import { La| } from './layout'", &grammar), expected);
        let wrapped = at("import {\n  La| \n} from './layout'", &grammar);
        assert!(
            matches!(&wrapped, Some(Context::Named { module, .. }) if module == "./layout"),
            "{wrapped:?}"
        );
        let second = at("import { A, La| } from './layout'", &grammar);
        assert!(
            matches!(&second, Some(Context::Named { module, .. }) if module == "./layout"),
            "{second:?}"
        );
    }

    #[test]
    fn a_word_between_the_keyword_and_the_brace_is_stepped_over() {
        // Scenario 14: `import type { … }`.
        let found = at("import type { La| } from './layout'", &typescript());
        assert!(matches!(&found, Some(Context::Named { module, .. }) if module == "./layout"));
    }

    #[test]
    fn a_list_with_no_module_yet_is_not_a_context() {
        // Scenario 15: there is nothing for a list to be an answer about.
        assert_eq!(at("import { La| }", &typescript()), None);
        // And it must not reach forward into the next statement for one.
        assert_eq!(at("import { La| }\nimport { B } from './other'", &typescript()), None);
    }

    #[test]
    fn an_ordinary_brace_is_not_an_import_list() {
        // The filter that matters most: only whitespace, words and commas may sit between the
        // keyword and the brace, so a function body and a class body are both refused.
        let grammar = typescript();
        assert_eq!(at("function draw() {\n  const la| = 1;\n}", &grammar), None);
        assert_eq!(at("class Layout {\n  la|\n}", &grammar), None);
    }

    #[test]
    fn a_language_that_named_no_imports_never_has_a_context() {
        // Scenario 18.
        let plain = Grammar { strings: vec!['\''], ..Grammar::default() };
        assert_eq!(at("import { A } from './la|'", &plain), None);
    }

    #[test]
    fn a_module_path_is_read_back_to_its_keyword() {
        // Scenarios 19, 20 and 21.
        let grammar = rust();
        assert_eq!(
            at("use quill_core::comp|", &grammar),
            Some(Context::Segment { segments: vec!["quill_core".to_owned()], stem: 16..20 })
        );
        assert_eq!(at("use |", &grammar), Some(Context::Segment { segments: vec![], stem: 4..4 }));
        assert_eq!(
            at("use quill_core::|", &grammar),
            Some(Context::Segment { segments: vec!["quill_core".to_owned()], stem: 16..16 })
        );
    }

    #[test]
    fn a_braced_list_is_stepped_over_to_the_path_that_owns_it() {
        // Scenarios 22 and 23.
        let grammar = rust();
        let nested = at("use quill_core::completion::{Candidate, R|}", &grammar);
        assert!(
            matches!(&nested, Some(Context::Segment { segments, .. })
                if segments == &["quill_core".to_owned(), "completion".to_owned()]),
            "{nested:?}"
        );
        let deeper = at("use quill_core::{a, b::c|}", &grammar);
        assert!(
            matches!(&deeper, Some(Context::Segment { segments, .. })
                if segments == &["quill_core".to_owned(), "b".to_owned()]),
            "{deeper:?}"
        );
    }

    #[test]
    fn a_visibility_in_front_of_the_keyword_changes_nothing() {
        // Scenario 24.
        let found = at("pub use quill_core::comp|", &rust());
        assert!(matches!(&found, Some(Context::Segment { segments, .. })
            if segments == &["quill_core".to_owned()]));
    }

    #[test]
    fn an_ordinary_path_in_code_is_not_an_import() {
        // Scenarios 25 and 26: the anchor is the whole of what makes this trustworthy.
        let grammar = rust();
        assert_eq!(at("let x = a::b::c|", &grammar), None);
        assert_eq!(at("use a::b as c|", &grammar), None);
        assert_eq!(at("    self.files.at(index).pa|", &grammar), None);
    }

    #[test]
    fn a_path_written_across_lines_still_reaches_its_keyword() {
        let found = at("use quill_core::{\n    completion,\n    symb|,\n};", &rust());
        assert!(
            matches!(&found, Some(Context::Segment { segments, .. })
                if segments == &["quill_core".to_owned()]),
            "{found:?}"
        );
    }

    #[test]
    fn the_two_acceptance_ranges_differ_only_for_a_specifier() {
        // §5.4: `Tab` replaces the whole of a specifier because the grammar cannot say what the
        // whole of one is; a segment and a named import are ordinary identifiers and say so.
        let sample = "import { A } from './la|yout'";
        let context = at(sample, &typescript()).expect("a specifier");
        let text = sample.replace('|', "");
        assert_eq!(&text[context.typed_range()], "./la");
        assert_eq!(&text[context.whole_range().expect("a specifier")], "./layout");
        let path = at("use quill_core::comp|", &rust()).expect("a segment");
        assert_eq!(path.whole_range(), None, "an identifier is left to completion::word_at");
    }

    #[test]
    fn reading_the_same_text_twice_gives_the_same_answer() {
        let grammar = typescript();
        let sample = "import {\n  Lay|out,\n  Caret,\n} from './layout'";
        assert_eq!(at(sample, &grammar), at(sample, &grammar));
    }

    #[test]
    fn a_caret_past_the_end_or_inside_a_character_answers_nothing() {
        let grammar = typescript();
        assert_eq!(context_at("import { A } from './a'", 900, &grammar), None);
        // The middle of a two-byte character, which a byte offset from a command line can be.
        assert_eq!(context_at("import { \u{00e9} } from './a'", 10, &grammar), None);
    }
}
