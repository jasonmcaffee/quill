//! What is in a database, as the tree draws it.
//!
//! One set of values for both engines, so `components::database::tree` has no idea which engine a row
//! came from — the seam `unluminous_core::mermaid::Scene` is for diagrams and `unluminous_chat::Reply` is for
//! the five wire shapes, made once more here.
//!
//! **Everything is asked for one level at a time.** Opening a data source lists its schemas, opening a
//! schema lists its tables, opening a table lists its columns. A database with four thousand tables in
//! it is the reason: The reference editor's own answer to that is an introspection-level setting, and asking
//! lazily is the same answer with nothing to configure.

use crate::value::Column;

/// What kind of thing a row in the tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Table,
    View,
    /// A materialised view, which is a view that holds its rows and therefore reads like a table.
    MaterialisedView,
    /// A table that lives somewhere else, through a foreign data wrapper.
    Foreign,
    Index,
    Sequence,
    Routine,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Table => "table",
            Kind::View => "view",
            Kind::MaterialisedView => "materialised view",
            Kind::Foreign => "foreign table",
            Kind::Index => "index",
            Kind::Sequence => "sequence",
            Kind::Routine => "routine",
        }
    }

    /// Which folder in the tree it hangs under, which is the reference editor's grouping.
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Table | Kind::Foreign => "tables",
            Kind::View | Kind::MaterialisedView => "views",
            Kind::Index => "indexes",
            Kind::Sequence => "sequences",
            Kind::Routine => "routines",
        }
    }

    /// Whether rows can be read out of it at all, which is what decides whether double-clicking it
    /// opens a grid.
    pub fn holds_rows(self) -> bool {
        matches!(self, Kind::Table | Kind::View | Kind::MaterialisedView | Kind::Foreign)
    }

    /// Whether rows in it can be **changed**, before the key question is even asked. A view's rows
    /// belong to the tables underneath it.
    pub fn can_be_changed(self) -> bool {
        matches!(self, Kind::Table)
    }
}

/// One thing in a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub kind: Kind,
}

/// A table or view, once its columns have been asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// Empty for SQLite, which has no schemas in the PostgreSQL sense.
    pub schema: String,
    pub name: String,
    pub columns: Vec<Column>,
    /// The columns of the primary key, in key order.
    ///
    /// **This is what decides whether a row can be changed**, and it is why it is a list rather than
    /// a flag: a compound key needs every part of itself in the `WHERE` clause, and matching on one
    /// of two would update the wrong rows.
    pub key: Vec<String>,
}

impl Table {
    /// The name to put in a statement, quoted so that a name needing quotes works and a name that
    /// does not is unchanged in the console's own history.
    pub fn qualified(&self, quote: char) -> String {
        match self.schema.is_empty() {
            true => quoted(&self.name, quote),
            false => format!("{}.{}", quoted(&self.schema, quote), quoted(&self.name, quote)),
        }
    }

    /// Whether a row here can be addressed on its own.
    ///
    /// The whole of the editing rule: a row can only be changed if there is something that names it
    /// and nothing else. Anything else means an `UPDATE` matching on every column, which quietly
    /// changes two identical rows — see `tasks/task-1777-database-plugin-tdd.md` §6.3.
    pub fn can_be_changed(&self) -> bool {
        !self.key.is_empty() && self.key.iter().all(|name| self.columns.iter().any(|column| column.name == *name))
    }

    /// Why not, for the line the grid shows in place of the buttons it does not draw.
    pub fn why_not_changeable(&self) -> Option<String> {
        match self.can_be_changed() {
            true => None,
            false => Some(format!(
                "`{}` has no primary key, so there is no way to change one row without changing \
                 every row that looks like it.",
                self.name
            )),
        }
    }
}

/// A name with quotes round it, doubling any quote already inside it.
///
/// Identifiers are the one place this crate builds SQL text rather than binding a parameter, because a
/// table name cannot be a parameter in any engine. Doubling is the escape both engines use, and the
/// quote character differs — `"` for PostgreSQL and SQLite, which is why it is an argument rather than
/// a constant.
pub fn quoted(name: &str, quote: char) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push(quote);
    for character in name.chars() {
        if character == quote {
            out.push(quote);
        }
        out.push(character);
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_table(key: &[&str]) -> Table {
        Table {
            schema: "public".to_owned(),
            name: "member".to_owned(),
            columns: vec![Column::new("id", "int4"), Column::new("name", "text")],
            key: key.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn a_table_with_no_key_cannot_be_changed_and_says_why() {
        assert!(a_table(&["id"]).can_be_changed());
        let no_key = a_table(&[]);
        assert!(!no_key.can_be_changed());
        assert!(no_key.why_not_changeable().unwrap().contains("no primary key"));
    }

    #[test]
    fn a_key_naming_a_column_that_is_not_there_is_not_a_key() {
        // Which happens when the columns were fetched and the key came from a stale read: better to
        // draw a read-only grid than to write a `WHERE` clause naming a column that does not exist.
        assert!(!a_table(&["id", "tenant"]).can_be_changed());
    }

    #[test]
    fn a_name_is_quoted_and_a_quote_inside_it_is_doubled() {
        assert_eq!(quoted("member", '"'), "\"member\"");
        assert_eq!(quoted("odd\"name", '"'), "\"odd\"\"name\"");
        assert_eq!(a_table(&["id"]).qualified('"'), "\"public\".\"member\"");
        assert_eq!(
            Table { schema: String::new(), name: "notes".to_owned(), ..Table::default() }.qualified('"'),
            "\"notes\""
        );
    }

    #[test]
    fn a_kind_says_where_it_hangs_and_whether_its_rows_can_be_changed() {
        assert_eq!(Kind::Table.folder(), "tables");
        assert_eq!(Kind::MaterialisedView.folder(), "views");
        assert!(Kind::View.holds_rows());
        // A view's rows belong to the tables underneath it, so they are read here and changed there.
        assert!(!Kind::View.can_be_changed());
        assert!(Kind::Table.can_be_changed());
        assert!(!Kind::Sequence.holds_rows());
    }
}
