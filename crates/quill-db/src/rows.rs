//! What a statement answered.
//!
//! One value for both engines and for both kinds of statement, because the grid draws one thing: a
//! `SELECT` fills [`Rows::columns`] and [`Rows::rows`], and an `UPDATE` fills [`Rows::affected`] and
//! leaves the grid empty. `Rows::tag` is the engine's own word for what happened — `SELECT 27`,
//! `UPDATE 1` — quoted rather than reconstructed, which is the rule `quill-git` keeps about a
//! program's own words.

use std::time::Duration;

use crate::value::{Column, Value};

/// The result of one statement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rows {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    /// How many rows the statement changed, when it said so. `None` for a `SELECT`.
    pub affected: Option<u64>,
    /// The engine's own completion tag.
    pub tag: String,
    /// How long the statement took, measured round the send and the read rather than reported by the
    /// server, because that is the number a person is actually waiting for.
    pub elapsed: Duration,
    /// True when more rows were available and the limit stopped them arriving.
    ///
    /// **Asked for as `limit + 1` and cut back**, which is what makes `1-200 of 200+` honest: nobody
    /// counted the rest, and a count that claims to be exact when the server was never asked for one
    /// is a lie the grid would be telling on every page.
    pub more: bool,
    /// Anything the server said that was not an error: a `NOTICE`, a `WARNING`, a raised message.
    ///
    /// Kept rather than dropped, because a `NOTICE` is often the only explanation of why a statement
    /// that succeeded did not do what was meant.
    pub notices: Vec<String>,
}

impl Rows {
    /// An empty result carrying only a tag, which is what a statement that returns no rows answers.
    pub fn of(tag: impl Into<String>, affected: Option<u64>) -> Self {
        Self { tag: tag.into(), affected, ..Self::default() }
    }

    /// Which column is called this, if any. Compared exactly, because SQL identifiers that differ in
    /// case are different names once they have been quoted.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name == name)
    }

    /// One line for the status bar and for `Output`.
    ///
    /// The engine's tag when there is one, since `SELECT 27` and `INSERT 0 1` are what somebody
    /// reading a console expects to see.
    pub fn summary(&self) -> String {
        let millis = self.elapsed.as_millis();
        match (self.tag.is_empty(), self.columns.is_empty()) {
            (false, _) => format!("{} in {millis} ms", self.tag),
            (true, false) => format!("{} rows in {millis} ms", self.rows.len()),
            (true, true) => format!("done in {millis} ms"),
        }
    }
}

/// What went wrong, in the engine's own words.
///
/// The message is the server's, verbatim. `code` is the five-character `SQLSTATE` where there is one,
/// because that is what tells a syntax error from a permission refusal from a server that is not
/// there, and `detail` and `hint` are PostgreSQL's own two extra fields — dropping them would throw
/// away the half of a Postgres error that says what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Failure {
    pub message: String,
    pub code: String,
    pub detail: String,
    pub hint: String,
    /// Where in the statement, when the server said. One-based, in characters.
    pub position: Option<u32>,
}

impl Failure {
    pub fn said(message: impl Into<String>) -> Self {
        Self { message: message.into(), ..Self::default() }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }
}

impl std::fmt::Display for Failure {
    /// Everything the server said, on one line each, in the order somebody reads them.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}", self.message)?;
        if !self.code.is_empty() {
            write!(out, " [{}]", self.code)?;
        }
        if !self.detail.is_empty() {
            write!(out, "\n{}", self.detail)?;
        }
        if !self.hint.is_empty() {
            write!(out, "\n{}", self.hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {}

/// Everything in this crate answers with this.
pub type Answer<T> = Result<T, Failure>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_quotes_the_engines_own_tag() {
        let mut rows = Rows::of("UPDATE 3", Some(3));
        rows.elapsed = Duration::from_millis(12);
        assert_eq!(rows.summary(), "UPDATE 3 in 12 ms");
    }

    #[test]
    fn a_failure_carries_the_servers_own_three_fields() {
        // Dropping `detail` and `hint` throws away the half of a Postgres error that says what to do
        // about it, which is the whole reason they are separate fields on the wire.
        let failure = Failure {
            message: "column \"nmae\" does not exist".to_owned(),
            code: "42703".to_owned(),
            hint: "Perhaps you meant to reference the column \"name\".".to_owned(),
            ..Failure::default()
        };
        let said = failure.to_string();
        assert!(said.starts_with("column \"nmae\" does not exist [42703]"));
        assert!(said.contains("Perhaps you meant"));
    }
}
