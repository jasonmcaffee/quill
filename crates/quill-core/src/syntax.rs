//! Reading source code into coloured tokens.
//!
//! It lives here, next to the Markdown parser, for the same reason that one does: it reads text and
//! produces spans over it, it draws nothing, and its tests run with no window, no graphics card and
//! no fonts. Nothing here knows what a colour scheme is either — a [`Token`] is what a stretch of
//! text *is*, and the window decides what colour that is.
//!
//! ## What it does, and what it does not
//!
//! One linear pass, no regular expressions, no dependency. The rules are tried in this order, and
//! the order is the whole design:
//!
//! 1. A line comment runs to the end of the line.
//! 2. A block comment runs to its terminator.
//! 3. A string runs to its matching quote, and a backslash escapes the next character.
//! 4. A number is a run of digits, with an optional decimal point, a hexadecimal prefix, or a
//!    suffix such as `u32` or `px`.
//! 5. A word is a keyword or a builtin if it is in the grammar's list.
//! 6. A word directly followed by `(` is a function; one starting with a capital letter is a type.
//! 7. Anything else is text.
//!
//! Comments and strings win over everything, because a keyword inside a string is not a keyword.
//!
//! Rules 6 is a **heuristic and is meant to be one**. `Promise.all(` colours `all` as a function and
//! `Promise` as a type without Quill understanding a single thing about JavaScript, which is what
//! `task-1649` asks for and is what a colouring pass is for. Real understanding is a language
//! server, and that is not what this is.
//!
//! Deliberately not handled, so nobody has to discover it: nested block comments in Rust — the first
//! terminator ends the comment; interpolation inside a template literal, which is coloured as part
//! of the string; JSX, which is text; and regular expression literals, which cannot be told from
//! division without parsing.

use std::ops::Range;

/// What a stretch of source is. The window turns this into a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// A word the language reserves: `fn`, `const`, `return`.
    Keyword,
    /// A name the language provides: `console`, `Vec`, `Promise`.
    Builtin,
    /// A word directly followed by an opening bracket.
    Function,
    /// A word starting with a capital letter.
    Type,
    String,
    Number,
    Comment,
    /// One of the grammar's operator characters.
    Operator,
    /// Everything else.
    Text,
}

impl Token {
    /// The name a colour scheme uses for this token in a plugin's manifest.
    pub fn name(self) -> &'static str {
        match self {
            Token::Keyword => "keyword",
            Token::Builtin => "builtin",
            Token::Function => "function",
            Token::Type => "type",
            Token::String => "string",
            Token::Number => "number",
            Token::Comment => "comment",
            Token::Operator => "operator",
            Token::Text => "text",
        }
    }

    pub const ALL: [Token; 9] = [
        Token::Keyword,
        Token::Builtin,
        Token::Function,
        Token::Type,
        Token::String,
        Token::Number,
        Token::Comment,
        Token::Operator,
        Token::Text,
    ];
}

/// How to read one language. Every field comes out of a plugin's manifest, so a language is data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grammar {
    /// The plugin this came from, for the status bar.
    pub language: String,
    /// The words the language reserves.
    pub keywords: Vec<String>,
    /// The names the language provides.
    pub builtins: Vec<String>,
    /// What starts a comment that runs to the end of the line, such as `//` or `#`.
    pub line_comment: Option<String>,
    /// What opens and closes a comment that spans lines.
    pub block_comment: Option<(String, String)>,
    /// The quote characters that open a string.
    pub strings: Vec<char>,
    /// True when a backslash inside a string escapes the next character.
    pub escapes: bool,
    /// The characters drawn as operators.
    pub operators: Vec<char>,
    /// True when a run of digits is a number.
    pub numbers: bool,
}

impl Grammar {
    /// Whether a word is one of the reserved ones.
    fn is_keyword(&self, word: &str) -> bool {
        self.keywords.iter().any(|known| known == word)
    }

    fn is_builtin(&self, word: &str) -> bool {
        self.builtins.iter().any(|known| known == word)
    }
}

/// Read `text` into coloured spans, in order and with no gaps between them that matter.
///
/// Only the spans that are not plain text are produced. Everything the window is not told about is
/// drawn in the document's own colour, which is what makes the result cheap to apply.
pub fn highlight(text: &str, grammar: &Grammar) -> Vec<(Range<usize>, Token)> {
    let bytes = text.as_bytes();
    let mut spans: Vec<(Range<usize>, Token)> = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &text[at..];
        if let Some(length) = comment(rest, grammar) {
            spans.push((at..at + length, Token::Comment));
            at += length;
            continue;
        }
        if let Some(length) = string(rest, grammar) {
            spans.push((at..at + length, Token::String));
            at += length;
            continue;
        }
        if grammar.numbers {
            if let Some(length) = number(rest) {
                spans.push((at..at + length, Token::Number));
                at += length;
                continue;
            }
        }
        if let Some(length) = word_length(rest) {
            let word = &rest[..length];
            let token = classify(word, &rest[length..], grammar);
            if token != Token::Text {
                spans.push((at..at + length, token));
            }
            at += length;
            continue;
        }
        let character = rest.chars().next().unwrap_or(' ');
        if grammar.operators.contains(&character) {
            spans.push((at..at + character.len_utf8(), Token::Operator));
        }
        at += character.len_utf8();
    }
    spans
}

/// What a word is, once it has been read.
fn classify(word: &str, after: &str, grammar: &Grammar) -> Token {
    if grammar.is_keyword(word) {
        return Token::Keyword;
    }
    if grammar.is_builtin(word) {
        return Token::Builtin;
    }
    // Directly followed by an opening bracket, with no space between: `listByChat(` is a call, and
    // `if (` is not, because `if` was caught as a keyword above.
    if after.starts_with('(') {
        return Token::Function;
    }
    if word.chars().next().is_some_and(char::is_uppercase) {
        return Token::Type;
    }
    Token::Text
}

/// How long the comment at the start of `rest` is, if it starts with one.
fn comment(rest: &str, grammar: &Grammar) -> Option<usize> {
    if let Some(opener) = &grammar.line_comment {
        if rest.starts_with(opener.as_str()) {
            let end = rest.find('\n').unwrap_or(rest.len());
            return Some(end);
        }
    }
    let (open, close) = grammar.block_comment.as_ref()?;
    if !rest.starts_with(open.as_str()) {
        return None;
    }
    // The first terminator ends it. Rust nests block comments and this does not, which is written
    // down in the module comment and in the plugin's own description rather than left to be found.
    match rest[open.len()..].find(close.as_str()) {
        Some(at) => Some(open.len() + at + close.len()),
        None => Some(rest.len()),
    }
}

/// How long the string at the start of `rest` is, if it starts with one.
fn string(rest: &str, grammar: &Grammar) -> Option<usize> {
    let quote = rest.chars().next()?;
    if !grammar.strings.contains(&quote) {
        return None;
    }
    let mut length = quote.len_utf8();
    let mut escaped = false;
    for character in rest[length..].chars() {
        length += character.len_utf8();
        if escaped {
            escaped = false;
            continue;
        }
        if grammar.escapes && character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            return Some(length);
        }
        // A string that is not closed by the end of its line is almost always a quote in prose or a
        // half-typed line, and colouring the rest of the file as a string would be worse than
        // colouring one line wrongly. A quote that legitimately spans lines — a template literal or
        // a Python docstring — is the exception, and those are the ones a grammar marks as such.
        if character == '\n' && quote != '`' {
            return Some(length - 1);
        }
    }
    Some(length)
}

/// How long the number at the start of `rest` is, if it starts with one.
fn number(rest: &str) -> Option<usize> {
    let mut characters = rest.char_indices();
    let (_, first) = characters.next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let mut end = first.len_utf8();
    let mut seen_point = false;
    for (index, character) in characters {
        if character.is_ascii_alphanumeric() || character == '_' {
            end = index + character.len_utf8();
            continue;
        }
        // One decimal point, and only when a digit follows it, so `1.max(2)` is not `1.` and then a
        // word.
        if character == '.' && !seen_point && rest[index + 1..].starts_with(|next: char| next.is_ascii_digit())
        {
            seen_point = true;
            end = index + 1;
            continue;
        }
        break;
    }
    Some(end)
}

/// How long the word at the start of `rest` is, if it starts with one.
///
/// A word is a letter, an underscore or a dollar followed by letters, digits, underscores or
/// dollars. The dollar is there because `$state` is one word in JavaScript and colouring it as an
/// operator and then a word would look wrong.
fn word_length(rest: &str) -> Option<usize> {
    let mut characters = rest.char_indices();
    let (_, first) = characters.next()?;
    if !(first.is_alphabetic() || first == '_' || first == '$') {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, character) in characters {
        if character.is_alphanumeric() || character == '_' || character == '$' {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn javascript() -> Grammar {
        Grammar {
            language: "JavaScript".to_owned(),
            keywords: ["const", "let", "function", "return", "class", "async", "await", "import", "from"]
                .iter()
                .map(|word| (*word).to_owned())
                .collect(),
            builtins: ["console", "Promise", "JSON"].iter().map(|word| (*word).to_owned()).collect(),
            line_comment: Some("//".to_owned()),
            block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            strings: vec!['"', '\'', '`'],
            escapes: true,
            operators: "+-*/=<>!&|?:;,.".chars().collect(),
            numbers: true,
        }
    }

    /// The tokens a piece of source produces, as text, so a test reads like the thing it is about.
    fn tokens(text: &str) -> Vec<(String, Token)> {
        highlight(text, &javascript())
            .into_iter()
            .map(|(range, token)| (text[range].to_owned(), token))
            .collect()
    }

    #[test]
    fn a_keyword_is_a_keyword_and_a_name_is_not() {
        let found = tokens("const total = 1;");
        assert_eq!(found[0], ("const".to_owned(), Token::Keyword));
        assert!(!found.iter().any(|(text, token)| text == "total" && *token == Token::Keyword));
    }

    #[test]
    fn a_keyword_inside_a_string_is_not_a_keyword() {
        // The whole reason strings are read before words.
        let found = tokens("const name = 'return the class';");
        assert!(found.contains(&("'return the class'".to_owned(), Token::String)));
        assert_eq!(
            found.iter().filter(|(_, token)| *token == Token::Keyword).count(),
            1,
            "only the const outside the string: {found:?}"
        );
    }

    #[test]
    fn a_keyword_inside_a_comment_is_not_a_keyword_either() {
        let found = tokens("// return the class\nconst x = 1;");
        assert!(found.contains(&("// return the class".to_owned(), Token::Comment)));
        assert_eq!(found.iter().filter(|(_, token)| *token == Token::Keyword).count(), 1);
    }

    #[test]
    fn a_block_comment_runs_to_its_terminator_and_no_further() {
        let found = tokens("/** Lists messages. */\nconst x = 1;");
        assert_eq!(found[0], ("/** Lists messages. */".to_owned(), Token::Comment));
        assert!(found.contains(&("const".to_owned(), Token::Keyword)));
    }

    #[test]
    fn a_block_comment_that_is_never_closed_takes_the_rest_of_the_file() {
        let found = tokens("/* forgot to close\nconst x = 1;");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, Token::Comment);
    }

    #[test]
    fn a_string_that_is_never_closed_stops_at_the_end_of_its_line() {
        // Colouring the rest of the file as a string because of one stray quote is worse than
        // colouring one line wrongly.
        let found = tokens("const a = 'oops\nconst b = 2;");
        assert!(found.contains(&("'oops".to_owned(), Token::String)), "{found:?}");
        assert_eq!(
            found.iter().filter(|(_, token)| *token == Token::Keyword).count(),
            2,
            "the const on the second line is still a keyword: {found:?}"
        );
    }

    #[test]
    fn a_backtick_string_may_span_lines_because_it_is_meant_to() {
        let found = tokens("const q = `select *\nfrom messages`;");
        assert!(found.iter().any(|(text, token)| *token == Token::String && text.contains('\n')));
    }

    #[test]
    fn a_backslash_escapes_the_quote_that_would_have_ended_the_string() {
        let found = tokens(r#"const a = "she said \"hello\" once";"#);
        let string = found.iter().find(|(_, token)| *token == Token::String).expect("a string");
        assert!(string.0.ends_with("once\""), "the whole string, not the first half: {string:?}");
    }

    #[test]
    fn a_word_directly_before_a_bracket_is_a_function() {
        let found = tokens("listByChat(chatId)");
        assert_eq!(found[0], ("listByChat".to_owned(), Token::Function));
        // With a space between, it is not a call: nothing in the line is coloured at all.
        let found = tokens("listByChat (chatId)");
        assert!(
            !found.iter().any(|(_, token)| *token == Token::Function),
            "a space before the bracket means it is not a call: {found:?}"
        );
    }

    #[test]
    fn a_word_starting_with_a_capital_is_a_type() {
        let found = tokens("const repository: MessageRepository = build();");
        assert!(found.contains(&("MessageRepository".to_owned(), Token::Type)));
        // A builtin wins over the capital letter rule, because the grammar named it.
        let found = tokens("Promise.resolve()");
        assert_eq!(found[0], ("Promise".to_owned(), Token::Builtin));
    }

    #[test]
    fn numbers_are_read_including_their_prefixes_and_suffixes() {
        let found = tokens("const a = 42; const b = 3.14; const c = 0xFF; const d = 10px;");
        let numbers: Vec<String> = found
            .iter()
            .filter(|(_, token)| *token == Token::Number)
            .map(|(text, _)| text.clone())
            .collect();
        assert_eq!(numbers, vec!["42", "3.14", "0xFF", "10px"]);
    }

    #[test]
    fn a_decimal_point_that_is_a_method_call_is_not_part_of_the_number() {
        let found = tokens("1.max(2)");
        assert_eq!(found[0], ("1".to_owned(), Token::Number));
        assert!(found.contains(&("max".to_owned(), Token::Function)));
    }

    #[test]
    fn a_word_may_start_with_a_dollar_or_an_underscore() {
        let found = tokens("const $state = _private;");
        let names: Vec<&String> = found.iter().map(|(text, _)| text).collect();
        assert!(!names.iter().any(|text| *text == "$"), "the dollar is part of the word: {found:?}");
    }

    #[test]
    fn every_span_is_inside_the_text_and_they_do_not_overlap() {
        let text = "/** doc */\nasync function listByChat(chatId) {\n  return `${chatId}`; // done\n}\n";
        let spans = highlight(text, &javascript());
        let mut previous = 0;
        for (range, _) in &spans {
            assert!(range.start >= previous, "spans overlap at {range:?}");
            assert!(range.end <= text.len(), "a span runs past the end of the text");
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
            previous = range.end;
        }
        assert!(!spans.is_empty());
    }

    #[test]
    fn text_with_no_grammar_behind_it_produces_nothing() {
        let empty = Grammar::default();
        assert!(highlight("const x = 1;", &empty).is_empty());
    }

    #[test]
    fn a_file_with_nothing_in_it_produces_nothing() {
        assert!(highlight("", &javascript()).is_empty());
    }

    #[test]
    fn a_hash_comment_works_as_well_as_a_slash_one() {
        let python = Grammar {
            line_comment: Some("#".to_owned()),
            keywords: vec!["def".to_owned()],
            strings: vec!['"', '\''],
            ..Grammar::default()
        };
        let text = "# a note\ndef thing():";
        let spans = highlight(text, &python);
        assert_eq!(&text[spans[0].0.clone()], "# a note");
        assert_eq!(spans[0].1, Token::Comment);
    }

    #[test]
    fn every_token_has_a_name_a_manifest_can_use() {
        for token in Token::ALL {
            assert!(!token.name().is_empty());
        }
        // The names are what a plugin's `theme.` keys are, so two tokens sharing one would make a
        // colour scheme ambiguous.
        let mut names: Vec<&str> = Token::ALL.iter().map(|token| token.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
