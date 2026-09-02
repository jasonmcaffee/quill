//! Where one statement ends and the next begins.
//!
//! A console holds several statements and Execute runs **the one under the caret**, which is
//! IntelliJ's behaviour and the only one that makes a console of six statements usable. So something
//! has to know where the boundaries are, and a `;` is not enough on its own: it appears inside string
//! literals, inside quoted identifiers, inside dollar-quoted function bodies, and inside both kinds of
//! comment.
//!
//! **This is not the colouring question and it is not asked of the tokeniser.** `quill_core::syntax`
//! knows about those four things too, but it answers "what colour is this run of characters" for a
//! plugin-described language, and a `.sql` plugin's grammar has no way to describe a dollar-quoted
//! body. The boundary question is small, exact, and belongs to the engine rather than to a manifest.

/// One statement in a console: where it starts, where it ends, and its text with nothing trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The byte offset of the first character.
    pub start: usize,
    /// The byte offset one past the last character, **including** the semicolon when there is one.
    pub end: usize,
    pub text: String,
}

impl Statement {
    /// The statement with its trailing semicolon and surrounding space taken off, which is what goes
    /// on the wire. PostgreSQL accepts either; SQLite's `prepare` stops at the first one.
    pub fn to_send(&self) -> &str {
        self.text.trim().trim_end_matches(';').trim_end()
    }

    /// True when there is nothing here but space and comments, which is what stops Execute sending an
    /// empty query because the caret was on a blank line between two statements.
    pub fn is_blank(&self) -> bool {
        strip_comments(&self.text).trim().trim_matches(';').trim().is_empty()
    }

    /// The first word, upper-cased, which is what decides whether a statement returns rows and
    /// whether a read-only source will take it.
    pub fn verb(&self) -> String {
        let text = strip_comments(&self.text);
        text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .find(|word| !word.is_empty())
            .unwrap_or_default()
            .to_ascii_uppercase()
    }
}

/// Every statement in the text, in order.
pub fn split(text: &str) -> Vec<Statement> {
    let bytes = text.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match skip_over(text, index) {
            // A run of characters that has to be passed over as a unit: a literal, an identifier, a
            // comment or a dollar-quoted body. A `;` inside one of these is not a boundary.
            Some(after) => index = after,
            None => {
                if bytes[index] == b';' {
                    let end = index + 1;
                    statements.push(Statement { start, end, text: text[start..end].to_owned() });
                    start = end;
                }
                index += 1;
            }
        }
    }
    if start < text.len() {
        statements.push(Statement { start, end: text.len(), text: text[start..].to_owned() });
    }
    statements
}

/// The statement the caret is inside, by byte offset.
///
/// A caret at the very end of a statement — just after its semicolon — belongs to **that** statement
/// rather than to the next one, because putting the caret at the end of a line and pressing Execute is
/// how most people run the thing they have just typed. A caret in the blank run after it belongs to
/// nothing, and Execute then looks backwards for the last statement that is not blank, which is what
/// `at` does by ignoring blank ones.
pub fn at(text: &str, caret: usize) -> Option<Statement> {
    let statements = split(text);
    let mut best: Option<Statement> = None;
    for statement in statements {
        if statement.is_blank() {
            continue;
        }
        if caret >= statement.start && caret <= statement.end {
            return Some(statement);
        }
        if statement.end <= caret {
            best = Some(statement);
        }
    }
    best
}

/// How far to skip if `index` starts something a `;` can hide inside, or `None` if it does not.
fn skip_over(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    match bytes[index] {
        // A single-quoted literal. Two quotes in a row are one quote, which is SQL's own escape.
        b'\'' => Some(closing(bytes, index, b'\'')),
        // A double-quoted identifier, escaped the same way.
        b'"' => Some(closing(bytes, index, b'"')),
        // MySQL-style backquoted identifiers are not this crate's engines, but skipping them costs a
        // line and stops a pasted statement splitting in the wrong place.
        b'`' => Some(closing(bytes, index, b'`')),
        b'-' if bytes.get(index + 1) == Some(&b'-') => {
            Some(bytes[index..].iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |at| index + at))
        }
        b'/' if bytes.get(index + 1) == Some(&b'*') => {
            // Block comments nest in PostgreSQL, which is a real difference from C and the reason
            // this counts rather than looking for the first `*/`.
            let mut depth = 1;
            let mut at = index + 2;
            while at + 1 < bytes.len() {
                match (bytes[at], bytes[at + 1]) {
                    (b'/', b'*') => {
                        depth += 1;
                        at += 2;
                    }
                    (b'*', b'/') => {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            return Some(at);
                        }
                    }
                    _ => at += 1,
                }
            }
            Some(bytes.len())
        }
        b'$' => dollar_quoted(text, index),
        _ => None,
    }
}

/// A PostgreSQL dollar-quoted body: `$$ … $$` or `$tag$ … $tag$`.
///
/// The tag is any run of letters, digits and underscores not starting with a digit. This is the case
/// that makes a naive splitter cut a function definition in half, because a `CREATE FUNCTION` body is
/// full of semicolons.
fn dollar_quoted(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = index + 1;
    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
        at += 1;
    }
    if at >= bytes.len() || bytes[at] != b'$' {
        return None;
    }
    // A tag starting with a digit is not a tag: `$1$` is a parameter followed by something else.
    if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let tag = &text[index..=at];
    match text[at + 1..].find(tag) {
        Some(offset) => Some(at + 1 + offset + tag.len()),
        None => Some(bytes.len()),
    }
}

/// Where a quoted run ends, one past its closing quote.
fn closing(bytes: &[u8], index: usize, quote: u8) -> usize {
    let mut at = index + 1;
    while at < bytes.len() {
        if bytes[at] == quote {
            // Doubled means an escaped quote rather than the end.
            if bytes.get(at + 1) == Some(&quote) {
                at += 2;
                continue;
            }
            return at + 1;
        }
        // PostgreSQL's standard_conforming_strings is on by default, so a backslash is an ordinary
        // character in a literal. Treating it as an escape would end a literal in the wrong place for
        // every value containing a backslash, which on Windows is every path.
        at += 1;
    }
    bytes.len()
}

/// The text with its comments replaced by spaces, so a word count reads the statement rather than the
/// prose around it. Offsets are preserved, which is what lets a caller keep using them.
pub fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let is_comment = (bytes[index] == b'-' && bytes.get(index + 1) == Some(&b'-'))
            || (bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*'));
        match skip_over(text, index) {
            Some(after) if is_comment => {
                out.extend(text[index..after].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
                index = after;
            }
            Some(after) => {
                out.push_str(&text[index..after]);
                index = after;
            }
            None => {
                let character = text[index..].chars().next().unwrap_or(' ');
                out.push(character);
                index += character.len_utf8();
            }
        }
    }
    out
}

/// Whether a statement only reads.
///
/// Used to say *before* sending it that a read-only data source will not take it — but it is never the
/// guarantee. The guarantee is the server's: a read-only PostgreSQL connection is put into a read-only
/// session and SQLite is opened read-only, so a statement this function got wrong is still refused by
/// the engine. See `tasks/task-1777-database-plugin-tdd.md` §7.
pub fn only_reads(statement: &Statement) -> bool {
    matches!(
        statement.verb().as_str(),
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "VALUES" | "TABLE" | "PRAGMA" | "DESCRIBE" | "ANALYZE"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str) -> Vec<String> {
        split(text).into_iter().map(|statement| statement.to_send().to_owned()).collect()
    }

    #[test]
    fn two_statements_are_two() {
        assert_eq!(texts("select 1; select 2;"), ["select 1", "select 2"]);
        // And the last one needs no semicolon, because a console usually has none on the line being
        // typed.
        assert_eq!(texts("select 1;\nselect 2"), ["select 1", "select 2"]);
    }

    #[test]
    fn a_semicolon_inside_a_string_is_not_a_boundary() {
        assert_eq!(texts("select 'a;b'"), ["select 'a;b'"]);
        // SQL escapes a quote by doubling it, so this is one literal and not two.
        assert_eq!(texts("select 'it''s; here'"), ["select 'it''s; here'"]);
        // And a backslash is an ordinary character, because standard_conforming_strings is on: a
        // reader that treated it as an escape would end the literal in the wrong place for every
        // Windows path anybody ever selected.
        assert_eq!(texts(r"select 'C:\dev\'; select 2"), [r"select 'C:\dev\'", "select 2"]);
    }

    #[test]
    fn a_semicolon_inside_a_quoted_identifier_or_a_comment_is_not_a_boundary() {
        assert_eq!(texts(r#"select "a;b" from t"#), [r#"select "a;b" from t"#]);
        assert_eq!(texts("select 1 -- a; comment\n"), ["select 1 -- a; comment"]);
        assert_eq!(texts("select /* a; b */ 1"), ["select /* a; b */ 1"]);
        // PostgreSQL's block comments nest, which C's do not.
        assert_eq!(texts("select /* a /* b; */ c; */ 1"), ["select /* a /* b; */ c; */ 1"]);
    }

    #[test]
    fn a_dollar_quoted_body_is_one_statement_however_many_semicolons_are_in_it() {
        // The case that makes a naive splitter cut a function definition in half.
        let text = "CREATE FUNCTION f() RETURNS int AS $$\nBEGIN\n  x := 1;\n  RETURN x;\nEND;\n$$ LANGUAGE plpgsql;\nSELECT f();";
        assert_eq!(split(text).len(), 2);
        assert!(split(text)[0].text.contains("RETURN x;"));
        assert_eq!(split(text)[1].to_send(), "SELECT f()");
        // A tagged body too.
        let tagged = "create function g() as $body$ select 1; $body$ language sql; select 2;";
        assert_eq!(split(tagged).len(), 2);
        // And `$1` is a parameter, not the start of a body.
        assert_eq!(texts("select $1; select $2"), ["select $1", "select $2"]);
    }

    #[test]
    fn the_statement_under_the_caret_is_the_one_it_is_inside() {
        let text = "select 1;\nselect 2;\nselect 3";
        assert_eq!(at(text, 0).unwrap().to_send(), "select 1");
        assert_eq!(at(text, 5).unwrap().to_send(), "select 1");
        // Just after the semicolon of the first is still the first, because putting the caret at the
        // end of the line you just typed and pressing Execute is how it is actually used.
        assert_eq!(at(text, 9).unwrap().to_send(), "select 1");
        assert_eq!(at(text, 12).unwrap().to_send(), "select 2");
        assert_eq!(at(text, text.len()).unwrap().to_send(), "select 3");
    }

    #[test]
    fn a_caret_in_the_blank_run_after_the_last_statement_runs_the_last_statement() {
        let text = "select 1;\n\n\n";
        assert_eq!(at(text, text.len()).unwrap().to_send(), "select 1");
        // And nothing at all answers nothing, rather than sending an empty query.
        assert!(at("\n\n-- just a comment\n", 4).is_none());
    }

    #[test]
    fn a_verb_is_read_past_the_comments_and_the_space() {
        assert_eq!(Statement { start: 0, end: 0, text: "  -- go\n  select 1".to_owned() }.verb(), "SELECT");
        assert_eq!(Statement { start: 0, end: 0, text: "/* x */ update t set a=1".to_owned() }.verb(), "UPDATE");
        assert_eq!(Statement { start: 0, end: 0, text: "\n\n".to_owned() }.verb(), "");
    }

    #[test]
    fn only_reads_answers_for_the_statements_a_console_actually_holds() {
        let of = |text: &str| Statement { start: 0, end: 0, text: text.to_owned() };
        assert!(only_reads(&of("select * from t")));
        assert!(only_reads(&of("WITH x AS (select 1) select * from x")));
        assert!(only_reads(&of("explain analyze select 1")));
        assert!(!only_reads(&of("update t set a = 1")));
        assert!(!only_reads(&of("drop table t")));
        // A `WITH … DELETE` reads as a read and is not one, which is exactly why this is never the
        // guarantee: the server's own read-only session is.
        assert!(only_reads(&of("with x as (delete from t returning *) select * from x")));
    }
}
