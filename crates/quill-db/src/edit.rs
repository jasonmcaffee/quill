//! Changes a person has made to a grid but not yet written.
//!
//! **A change is pending until it is submitted**, which is IntelliJ's arrangement and the right one:
//! typing in a grid should not send a statement per keystroke, and what has been written and what has
//! not should be visible without pressing anything.
//!
//! The whole of the safety of this file is in one rule, and it is `catalog::Table::can_be_changed`: a
//! row can only be changed if the table has a key that names it and nothing else. Everything here
//! composes a `WHERE` clause out of that key. There is no fallback that matches on every column,
//! because that is the fallback which quietly changes two identical rows.
//!
//! **Nothing is quoted by hand except an identifier**, which cannot be a parameter in any engine.
//! Every value goes down as a bound parameter, so a value containing a quote, a newline or a backslash
//! is a non-event.

use crate::catalog::{quoted, Table};
use crate::rows::{Answer, Failure};
use crate::value::Value;

/// Which row a change is to, by the values of its key.
///
/// The **original** values, kept from when the grid was filled, so that changing a key column still
/// finds the row it started as. A row added in the grid and not yet written has no key at all, which
/// is what [`Row::Added`] is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Row {
    /// A row that is in the database, named by its key.
    Keyed(Vec<String>),
    /// A row somebody added here, named by when they added it.
    Added(usize),
}

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Put a value in a cell.
    Set { row: Row, column: String, value: Value },
    /// Add a row, with whatever has been typed into it so far.
    Add { at: usize, values: Vec<(String, Value)> },
    /// Take a row away.
    Delete { row: Row },
}

/// Everything pending on one grid.
///
/// A list rather than a map, because the order matters: an insert and then an edit of the row it
/// added have to be applied in the order they were made, and a person undoing by hand expects the
/// last thing they did to be the last thing listed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pending {
    pub changes: Vec<Change>,
    /// What the next added row will be called.
    next: usize,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// Record a cell.
    ///
    /// **Setting a cell twice replaces the first**, rather than sending two `UPDATE`s: what somebody
    /// means by typing in a cell, changing their mind and typing again is one change.
    pub fn set(&mut self, row: Row, column: &str, value: Value) {
        // A cell of a row that is itself pending goes on that row rather than becoming an `UPDATE` of
        // something that is not there yet.
        if let Row::Added(added) = row {
            if let Some(Change::Add { values, .. }) = self.changes.iter_mut().find(
                |change| matches!(change, Change::Add { at, .. } if *at == added),
            ) {
                values.retain(|(name, _)| name != column);
                values.push((column.to_owned(), value));
                return;
            }
        }
        self.changes.retain(
            |change| !matches!(change, Change::Set { row: other, column: name, .. } if *other == row && name == column),
        );
        self.changes.push(Change::Set { row, column: column.to_owned(), value });
    }

    /// Record a new row, and answer which one it is.
    pub fn add(&mut self) -> Row {
        let at = self.next;
        self.next += 1;
        self.changes.push(Change::Add { at, values: Vec::new() });
        Row::Added(at)
    }

    /// Record a row going away.
    ///
    /// Deleting a row that was only ever pending **removes the pending row** rather than recording a
    /// `DELETE` of something the database has never heard of.
    pub fn delete(&mut self, row: Row) {
        if let Row::Added(added) = row {
            self.changes.retain(|change| !touches_added(change, added));
            return;
        }
        self.changes.retain(|change| !matches!(change, Change::Set { row: other, .. } if *other == row));
        if !self.changes.iter().any(|change| matches!(change, Change::Delete { row: other } if *other == row)) {
            self.changes.push(Change::Delete { row });
        }
    }

    /// Throw one change away, which is what Revert Selected does.
    pub fn revert(&mut self, row: &Row, column: Option<&str>) {
        match column {
            Some(column) => self.changes.retain(
                |change| !matches!(change, Change::Set { row: other, column: name, .. } if other == row && name == column),
            ),
            None => self.changes.retain(|change| !mentions(change, row)),
        }
    }

    /// The value pending in a cell, if there is one, for the grid to draw in place of what it read.
    pub fn value_of(&self, row: &Row, column: &str) -> Option<&Value> {
        self.changes.iter().rev().find_map(|change| match change {
            Change::Set { row: other, column: name, value } if other == row && name == column => Some(value),
            Change::Add { at, values } if *row == Row::Added(*at) => {
                values.iter().find(|(name, _)| name == column).map(|(_, value)| value)
            }
            _ => None,
        })
    }

    /// True when this row is going away when Submit is pressed.
    pub fn is_deleted(&self, row: &Row) -> bool {
        self.changes.iter().any(|change| matches!(change, Change::Delete { row: other } if other == row))
    }

    /// The rows that have been added and not yet written, in the order they were added.
    pub fn added(&self) -> Vec<usize> {
        self.changes
            .iter()
            .filter_map(|change| match change {
                Change::Add { at, .. } => Some(*at),
                _ => None,
            })
            .collect()
    }

    /// The statements these changes become, in order, with their parameters.
    ///
    /// **This is what Preview shows** — the actual statements rather than a summary of them — and it
    /// is exactly what Submit sends, from the same call, so the preview cannot drift from what
    /// happens.
    pub fn statements(&self, table: &Table, engine: crate::source::Engine) -> Answer<Vec<Statement>> {
        if let Some(why) = table.why_not_changeable() {
            return Err(Failure::said(why));
        }
        let mut out = Vec::new();
        // The sets of one added row are gathered onto its `INSERT` rather than following it as
        // updates, because the row has no key until it exists.
        for change in &self.changes {
            match change {
                Change::Add { at, values } => out.push(insert(table, engine, values, *at)?),
                Change::Set { row: Row::Added(_), .. } => {}
                Change::Set { row, column, value } => {
                    out.push(update(table, engine, row, column, value)?)
                }
                Change::Delete { row } => out.push(delete(table, engine, row)?),
            }
        }
        Ok(out)
    }
}

/// One statement and its parameters, ready to send and ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub values: Vec<Value>,
    /// One line for the preview, saying what it does in words.
    pub what: String,
}

/// `$1` for PostgreSQL and `?1` for SQLite, which is the one place the two differ in a statement this
/// composes.
fn placeholder(engine: crate::source::Engine, at: usize) -> String {
    match engine {
        crate::source::Engine::Postgres => format!("${at}"),
        crate::source::Engine::Sqlite => format!("?{at}"),
    }
}

fn update(
    table: &Table,
    engine: crate::source::Engine,
    row: &Row,
    column: &str,
    value: &Value,
) -> Answer<Statement> {
    let key = key_values(table, row)?;
    let mut values = vec![value.clone()];
    let mut at = 2;
    let where_clause = table
        .key
        .iter()
        .zip(key.iter())
        .map(|(name, value)| {
            values.push(Value::typed(value));
            let piece = format!("{} = {}", quoted(name, '"'), placeholder(engine, at));
            at += 1;
            piece
        })
        .collect::<Vec<String>>()
        .join(" AND ");
    Ok(Statement {
        sql: format!(
            "UPDATE {} SET {} = {} WHERE {where_clause}",
            table.qualified('"'),
            quoted(column, '"'),
            placeholder(engine, 1)
        ),
        values,
        what: format!("set {column} on the row where {}", named(table, &key)),
    })
}

fn delete(table: &Table, engine: crate::source::Engine, row: &Row) -> Answer<Statement> {
    let key = key_values(table, row)?;
    let mut values = Vec::new();
    let mut at = 1;
    let where_clause = table
        .key
        .iter()
        .zip(key.iter())
        .map(|(name, value)| {
            values.push(Value::typed(value));
            let piece = format!("{} = {}", quoted(name, '"'), placeholder(engine, at));
            at += 1;
            piece
        })
        .collect::<Vec<String>>()
        .join(" AND ");
    Ok(Statement {
        sql: format!("DELETE FROM {} WHERE {where_clause}", table.qualified('"')),
        values,
        what: format!("delete the row where {}", named(table, &key)),
    })
}

fn insert(
    table: &Table,
    engine: crate::source::Engine,
    values: &[(String, Value)],
    at: usize,
) -> Answer<Statement> {
    // A row with nothing typed into it is `DEFAULT VALUES`, which is a real statement in both engines
    // and the honest translation of "add a row and leave every column alone".
    if values.is_empty() {
        return Ok(Statement {
            sql: format!("INSERT INTO {} DEFAULT VALUES", table.qualified('"')),
            values: Vec::new(),
            what: format!("add row {} with every column at its default", at + 1),
        });
    }
    let names = values
        .iter()
        .map(|(name, _)| quoted(name, '"'))
        .collect::<Vec<String>>()
        .join(", ");
    let marks = (1..=values.len())
        .map(|at| placeholder(engine, at))
        .collect::<Vec<String>>()
        .join(", ");
    Ok(Statement {
        sql: format!("INSERT INTO {} ({names}) VALUES ({marks})", table.qualified('"')),
        values: values.iter().map(|(_, value)| value.clone()).collect(),
        what: format!("add a row with {}", values.iter().map(|(name, _)| name.as_str()).collect::<Vec<&str>>().join(", ")),
    })
}

/// The key values of a row, refusing a row that has none.
fn key_values(table: &Table, row: &Row) -> Answer<Vec<String>> {
    match row {
        Row::Keyed(values) if values.len() == table.key.len() => Ok(values.clone()),
        Row::Keyed(values) => Err(Failure::said(format!(
            "that row is named by {} value(s) and `{}`'s key has {} column(s), so the two do not \
             describe the same row. Reload the grid.",
            values.len(),
            table.name,
            table.key.len()
        ))),
        Row::Added(_) => Err(Failure::said(
            "that row has not been written yet, so there is nothing in the database to change.",
        )),
    }
}

/// `id = 4`, for the sentence a preview row shows.
fn named(table: &Table, key: &[String]) -> String {
    table
        .key
        .iter()
        .zip(key.iter())
        .map(|(name, value)| format!("{name} = {value}"))
        .collect::<Vec<String>>()
        .join(" and ")
}

fn mentions(change: &Change, row: &Row) -> bool {
    match change {
        Change::Set { row: other, .. } | Change::Delete { row: other } => other == row,
        Change::Add { at, .. } => *row == Row::Added(*at),
    }
}

fn touches_added(change: &Change, added: usize) -> bool {
    match change {
        Change::Add { at, .. } => *at == added,
        Change::Set { row, .. } | Change::Delete { row } => *row == Row::Added(added),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Engine;
    use crate::value::Column;

    fn member() -> Table {
        Table {
            schema: "public".to_owned(),
            name: "member".to_owned(),
            columns: vec![Column::new("id", "int4"), Column::new("name", "text")],
            key: vec!["id".to_owned()],
        }
    }

    fn keyed(id: &str) -> Row {
        Row::Keyed(vec![id.to_owned()])
    }

    #[test]
    fn a_cell_becomes_an_update_matching_on_the_key_and_nothing_else() {
        let mut pending = Pending::default();
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        let statements = pending.statements(&member(), Engine::Postgres).expect("statements");
        assert_eq!(statements.len(), 1);
        assert_eq!(statements[0].sql, "UPDATE \"public\".\"member\" SET \"name\" = $1 WHERE \"id\" = $2");
        assert_eq!(statements[0].values, [Value::typed("Ada"), Value::typed("4")]);
        // And SQLite's placeholders are its own.
        let sqlite = pending.statements(&member(), Engine::Sqlite).expect("statements");
        assert_eq!(sqlite[0].sql, "UPDATE \"public\".\"member\" SET \"name\" = ?1 WHERE \"id\" = ?2");
    }

    #[test]
    fn nothing_is_quoted_by_hand_so_an_awkward_value_is_a_non_event() {
        // The case that is a fault in every implementation that builds SQL by concatenation.
        let mut pending = Pending::default();
        let awkward = "it's \"quoted\"; -- \\ and a\nnewline";
        pending.set(keyed("4"), "name", Value::typed(awkward));
        let statements = pending.statements(&member(), Engine::Postgres).expect("statements");
        assert!(!statements[0].sql.contains("it's"), "the value is not in the statement at all");
        assert_eq!(statements[0].values[0], Value::typed(awkward));
    }

    #[test]
    fn a_table_with_no_key_refuses_to_produce_a_statement_at_all() {
        // The whole of the safety of this file: there is no fallback that matches on every column,
        // because that is the fallback which quietly changes two identical rows.
        let no_key = Table { key: Vec::new(), ..member() };
        let mut pending = Pending::default();
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        let refused = pending.statements(&no_key, Engine::Postgres).expect_err("refused");
        assert!(refused.message.contains("no primary key"), "{refused}");
    }

    #[test]
    fn a_compound_key_puts_every_part_of_itself_in_the_where_clause() {
        // Matching on one of two parts would update the wrong rows, which is why the key is a list.
        let table = Table {
            key: vec!["tenant".to_owned(), "id".to_owned()],
            columns: vec![Column::new("tenant", "text"), Column::new("id", "int4"), Column::new("name", "text")],
            ..member()
        };
        let mut pending = Pending::default();
        pending.set(Row::Keyed(vec!["acme".to_owned(), "4".to_owned()]), "name", Value::typed("Ada"));
        let statements = pending.statements(&table, Engine::Postgres).expect("statements");
        assert!(statements[0].sql.ends_with("WHERE \"tenant\" = $2 AND \"id\" = $3"), "{}", statements[0].sql);
        assert_eq!(statements[0].values.len(), 3);
    }

    #[test]
    fn setting_one_cell_twice_is_one_update() {
        // What somebody means by typing, changing their mind and typing again.
        let mut pending = Pending::default();
        pending.set(keyed("4"), "name", Value::typed("Ad"));
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.value_of(&keyed("4"), "name"), Some(&Value::typed("Ada")));
    }

    #[test]
    fn an_added_row_gathers_its_cells_onto_one_insert() {
        let mut pending = Pending::default();
        let row = pending.add();
        pending.set(row.clone(), "id", Value::typed("9"));
        pending.set(row.clone(), "name", Value::typed("Grace"));
        let statements = pending.statements(&member(), Engine::Postgres).expect("statements");
        assert_eq!(statements.len(), 1, "one insert, not an insert and two updates");
        assert_eq!(statements[0].sql, "INSERT INTO \"public\".\"member\" (\"id\", \"name\") VALUES ($1, $2)");
        assert_eq!(statements[0].values, [Value::typed("9"), Value::typed("Grace")]);
    }

    #[test]
    fn an_added_row_with_nothing_typed_into_it_is_default_values() {
        let mut pending = Pending::default();
        pending.add();
        let statements = pending.statements(&member(), Engine::Postgres).expect("statements");
        assert_eq!(statements[0].sql, "INSERT INTO \"public\".\"member\" DEFAULT VALUES");
    }

    #[test]
    fn deleting_a_row_that_was_only_ever_pending_removes_it_rather_than_writing_a_delete() {
        let mut pending = Pending::default();
        let row = pending.add();
        pending.set(row.clone(), "name", Value::typed("Grace"));
        pending.delete(row);
        assert!(pending.is_empty(), "nothing to write: the row never existed");
    }

    #[test]
    fn deleting_a_real_row_throws_away_the_edits_to_it() {
        let mut pending = Pending::default();
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        pending.delete(keyed("4"));
        let statements = pending.statements(&member(), Engine::Postgres).expect("statements");
        assert_eq!(statements.len(), 1, "one delete, not an update of a row about to go");
        assert!(statements[0].sql.starts_with("DELETE FROM"));
        assert!(pending.is_deleted(&keyed("4")));
    }

    #[test]
    fn a_row_named_by_the_wrong_number_of_values_is_refused_rather_than_guessed_at() {
        let table = Table {
            key: vec!["tenant".to_owned(), "id".to_owned()],
            columns: vec![Column::new("tenant", "text"), Column::new("id", "int4"), Column::new("name", "text")],
            ..member()
        };
        let mut pending = Pending::default();
        // One value for a key of two: the row and the table do not describe the same thing, which
        // happens when the grid was filled before somebody changed the table underneath it.
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        let refused = pending.statements(&table, Engine::Postgres).expect_err("refused");
        assert!(refused.message.contains("do not describe the same row"), "{refused}");
    }

    #[test]
    fn revert_takes_back_one_cell_or_the_whole_row() {
        let mut pending = Pending::default();
        pending.set(keyed("4"), "name", Value::typed("Ada"));
        pending.set(keyed("4"), "id", Value::typed("5"));
        pending.revert(&keyed("4"), Some("name"));
        assert_eq!(pending.len(), 1);
        pending.revert(&keyed("4"), None);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_name_needing_quotes_is_quoted_and_a_quote_inside_it_is_doubled() {
        let table = Table { name: "odd\"name".to_owned(), ..member() };
        let mut pending = Pending::default();
        pending.set(keyed("4"), "a\"b", Value::Null);
        let statements = pending.statements(&table, Engine::Postgres).expect("statements");
        assert!(statements[0].sql.contains("\"odd\"\"name\""), "{}", statements[0].sql);
        assert!(statements[0].sql.contains("\"a\"\"b\""), "{}", statements[0].sql);
        assert_eq!(statements[0].values[0], Value::Null, "and NULL goes down as NULL");
    }
}
