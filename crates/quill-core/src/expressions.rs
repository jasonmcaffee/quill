//! What text a point in a file is a question about, when the question is *what does this hold*.
//!
//! `task-1696`: resting the pointer on a name while a program is paused shows its value, and what
//! The reference editor shows the value of is not the bare word — pointing at `count` in `self.items.count` asks
//! about `self.items.count`. This module is that reading, and nothing else: it takes the identifier
//! [`FileSymbols::identifier_at`] already answers with and extends it **backwards** over field
//! access.
//!
//! ## Backwards only
//!
//! The sub-expression that *ends* at the pointer is the thing being pointed at. Pointing at `items`
//! in `self.items.count` asks about `self.items`, which is what a person means and what the reference editor
//! does. Walking forwards as well would answer a different question from the one the pointer asked.
//!
//! ## Three things it will not cross, each for the same reason
//!
//! The tier is the syntactic one `task-1675`, `task-1680` and `task-1686` already chose, so the walk
//! **is** the parse and it stops wherever going on would need a real one:
//!
//! - **`::`** is a path rather than a field access. `std::env::args` names a module and a function,
//!   not a value, and a rule that read it as field access would hand the debugger a module name at
//!   every `use` line. A local is never reached through `::` in any language Quill ships a plugin
//!   for.
//! - **A bracket** — `items[0].name`, `f().x` — would mean matching brackets, and matching brackets
//!   is a parser. The walk stops at `name` and at `x`, which either resolve in the frame or do not.
//! - **A newline.** A field path broken over two lines is rare; a walk that went through one would
//!   pick up the tail of the line above every time the pointer sat on the first word of a line.
//!
//! The two separators are `.` and `->`, which is every spelling of field access there is, and
//! neither is a word character in any grammar — so unlike `task-1671`'s three keys and
//! `task-1680`'s nine, this needs nothing from the plugin.
//!
//! ## A keyword is not a question, even when it is a value
//!
//! `self` and `this` are values a debugger can answer about, and they are keywords, so a pointer
//! resting on one on its own asks nothing. The alternative is a list of the keywords that happen to
//! be values, which is a list of languages inside Quill — the exact thing `language.definers` and
//! the nine import keys exist to prevent. The cost is small: `self.items` is read whole, because the
//! **segment** walk reads the text rather than the identifier list.

use std::ops::Range;

use crate::symbols::FileSymbols;
use crate::syntax::Grammar;

/// The expression the point at `offset` is a question about.
///
/// `None` for a keyword, a number, an operator, and anywhere inside a comment or a string — which is
/// [`FileSymbols::identifier_at`]'s own floor, and the right one: a value tooltip over the word
/// `return`, or over a word in a doc comment, would be a promise with nothing behind it.
pub fn at(
    text: &str,
    symbols: &FileSymbols,
    grammar: &Grammar,
    offset: usize,
) -> Option<Range<usize>> {
    let word = symbols.identifier_at(offset)?;
    let mut start = word.start;
    while let Some(earlier) = one_step_back(text, grammar, start) {
        start = earlier;
    }
    Some(start..word.end)
}

/// One `<segment> <separator>` taken off the front, or nothing when there is not one there.
///
/// Returns where the segment it found begins, which is the new front of the expression.
///
/// The segment is read straight out of the text rather than out of [`FileSymbols`], and that is
/// deliberate: **`self` and `this` are keywords**, so they are not identifier tokens and the words
/// list does not hold them — and `self.items.count` is the single commonest shape this module
/// exists for. What a word is made of is still the language's own answer, through
/// [`Grammar::is_word_character`], which is the rule `task-1671` set when a hyphen became a letter
/// in CSS. The role — is this inside a comment or a string — was already settled at the pointer,
/// and a field path cannot begin inside a comment and end outside one.
fn one_step_back(text: &str, grammar: &Grammar, start: usize) -> Option<usize> {
    let before_separator = separator_before(text, start)?;
    let ends = skip_spaces_back(text, before_separator);
    let mut at = ends;
    while let Some(character) = text[..at].chars().next_back() {
        if !grammar.is_word_character(character, at == ends) {
            break;
        }
        at -= character.len_utf8();
    }
    // Nothing but the separator, so there is no segment in front of it: `.x` after a bracket, an
    // operator, or the start of a line.
    (at < ends).then_some(at)
}

/// Where a `.` or a `->` immediately before `start` begins, ignoring spaces and tabs.
fn separator_before(text: &str, start: usize) -> Option<usize> {
    let at = skip_spaces_back(text, start);
    let head = &text[..at];
    if let Some(rest) = head.strip_suffix("->") {
        return Some(rest.len());
    }
    let rest = head.strip_suffix('.')?;
    // `a..b` is a range and `a...b` is a range in three other languages. Neither is field access,
    // and both would otherwise look like one with an identifier that happens to end where the first
    // dot begins.
    (!rest.ends_with('.')).then_some(rest.len())
}

/// Back over spaces and tabs, but never over a line break.
fn skip_spaces_back(text: &str, mut at: usize) -> usize {
    while let Some(character) = text[..at].chars().next_back() {
        if character != ' ' && character != '\t' {
            break;
        }
        at -= character.len_utf8();
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Grammar;

    fn rust() -> Grammar {
        Grammar {
            keywords: ["fn", "let", "return", "self", "for", "in"]
                .iter()
                .map(|word| (*word).to_owned())
                .collect(),
            line_comment: Some("//".to_owned()),
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"'],
            escapes: true,
            numbers: true,
            operators: "+-*/=<>!&|%^~?:;,.()[]{}".chars().collect(),
            ..Grammar::default()
        }
    }

    /// The expression at the byte offset of the first occurrence of `needle`, plus `into` bytes.
    fn read(text: &str, needle: &str, into: usize) -> Option<String> {
        let grammar = rust();
        let symbols = FileSymbols::read(text, &grammar);
        let offset = text.find(needle).expect("the needle is in the text") + into;
        at(text, &symbols, &grammar, offset).map(|range| text[range].to_owned())
    }

    #[test]
    fn a_bare_word_is_itself() {
        assert_eq!(read("let total = 3;\n", "total", 1).as_deref(), Some("total"));
    }

    /// The whole point of the module: pointing at the last segment asks about the whole path.
    #[test]
    fn a_field_path_is_read_backwards_to_its_root() {
        let text = "let n = self.items.count;\n";
        assert_eq!(read(text, "count", 1).as_deref(), Some("self.items.count"));
        // And pointing at a middle segment asks about the path that **ends** there, which is what
        // the pointer is on.
        assert_eq!(read(text, "items", 1).as_deref(), Some("self.items"));
        // And `self` on its own is a keyword rather than an identifier, so the pointer resting on it
        // asks nothing. See the module comment: a list of the keywords that are values would be a
        // list of languages inside Quill, and the shape this module exists for reads it anyway.
        assert_eq!(read(text, "self", 1), None);
    }

    #[test]
    fn the_arrow_spelling_is_read_too() {
        assert_eq!(read("node->next->value;\n", "value", 1).as_deref(), Some("node->next->value"));
    }

    #[test]
    fn spaces_round_a_separator_are_ignored() {
        assert_eq!(read("shape . size;\n", "size", 1).as_deref(), Some("shape . size"));
    }

    /// `::` is a path, not a field access, and a rule that crossed it would hand a debugger a module
    /// name at every `use` line.
    #[test]
    fn a_path_separator_is_not_crossed() {
        assert_eq!(read("std::env::args();\n", "args", 1).as_deref(), Some("args"));
    }

    /// Matching brackets is a parser, and this tier does not have one.
    #[test]
    fn a_bracket_is_not_crossed() {
        assert_eq!(read("let x = items[0].name;\n", "name", 1).as_deref(), Some("name"));
        assert_eq!(read("let x = read().value;\n", "value", 1).as_deref(), Some("value"));
    }

    /// A walk through a line break would pick up the tail of the line above whenever the pointer sat
    /// on the first word of a line.
    #[test]
    fn a_line_break_is_not_crossed() {
        assert_eq!(read("let a = one.\n    two;\n", "two", 1).as_deref(), Some("two"));
    }

    /// `a..b` is a range in Rust and `a...b` in three other languages. Neither is field access.
    #[test]
    fn a_range_is_not_a_field_access() {
        assert_eq!(read("for i in start..finish {\n", "finish", 1).as_deref(), Some("finish"));
    }

    /// `identifier_at`'s own floor, kept: a keyword, a comment and a string are not questions about
    /// a value.
    #[test]
    fn a_keyword_a_comment_and_a_string_answer_nothing() {
        assert_eq!(read("return total;\n", "return", 1), None);
        assert_eq!(read("// the total here\n", "total", 1), None);
        assert_eq!(read("let s = \"total\";\n", "total", 1), None);
    }

    /// The point at the very end of a word is on it, which is where a caret lands after a double
    /// click and is what `Debug -> Show Value` asks from.
    #[test]
    fn the_point_at_the_end_of_a_word_is_on_it() {
        let text = "let n = self.items;\n";
        let offset = text.find("items").expect("the needle") + "items".len();
        let grammar = rust();
        let symbols = FileSymbols::read(text, &grammar);
        let range = at(text, &symbols, &grammar, offset).expect("an expression");
        assert_eq!(&text[range], "self.items");
    }
}
