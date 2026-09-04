//! Speaking to SQLite, which needs nothing new.
//!
//! `rusqlite` with SQLite's own C compiled in is already a workspace dependency, for the Agent-Tasks
//! board. A SQLite data source is a file: no server, no port, no password and no authentication, so
//! this file is the same four questions the PostgreSQL module answers, over a library call instead of
//! a socket.
//!
//! **Two things are genuinely different from PostgreSQL and both are improvements.** `sqlite_master`
//! holds the *original text* of every `CREATE` statement, so the DDL a table shows here is what was
//! typed rather than something composed from a catalogue. And every ordinary table has a `rowid` even
//! when it has no primary key, which means a row can be addressed — so a table PostgreSQL would have
//! to draw read-only is editable here, through a key nobody declared.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use crate::catalog::{Item, Kind, Table};
use crate::rows::{Answer, Failure, Rows};
use crate::value::{Column, Value};

/// The three names SQLite answers its implicit key by, in the order they are tried.
///
/// **Three rather than one, because a table may declare a real column called `rowid`.** SQLite lets
/// it, and once it does, `rowid` in a statement means that column — which need not be unique, so an
/// `UPDATE … WHERE "rowid" = ?` written against it would change every row sharing the value. That is
/// exactly the fault the whole editing rule exists to prevent, arriving through the one door that is
/// not a declared key. So the alias used is the first of these that the table has **not** shadowed,
/// and a table that has shadowed all three has no key at all and is drawn read only.
pub const ROWID_ALIASES: &[&str] = &["rowid", "_rowid_", "oid"];

/// The first of them, which is what an unshadowed table uses.
pub const ROWID: &str = "rowid";

/// One open database file.
pub struct Session {
    connection: Connection,
    file: PathBuf,
    read_only: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("SqliteSession")
            .field("file", &self.file)
            .field("read_only", &self.read_only)
            .finish()
    }
}

impl Session {
    /// Open a file.
    ///
    /// A read-only data source is opened with `SQLITE_OPEN_READONLY`, so the guarantee is SQLite's
    /// rather than a check in Unluminous — the same arrangement the PostgreSQL session makes with a
    /// read-only transaction session.
    ///
    /// **A file that is not there is not created.** `rusqlite`'s default flags include
    /// `SQLITE_OPEN_CREATE`, so opening a mistyped path would quietly make an empty database and the
    /// tree would show a data source with nothing in it rather than saying the file is missing.
    pub fn open(file: &Path, read_only: bool) -> Answer<Session> {
        let flags = match read_only {
            true => OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            false => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
        };
        if !file.exists() {
            return Err(Failure::said(format!("there is no file at {}.", file.display())));
        }
        let connection = Connection::open_with_flags(file, flags).map_err(said)?;
        // Foreign keys are off by default in SQLite, which means a `DELETE` that should have been
        // refused succeeds and leaves rows pointing at nothing. Turning them on is what every other
        // tool that edits a SQLite database does, and it is what makes a refusal here match the
        // schema the person wrote.
        let _ = connection.execute_batch("PRAGMA foreign_keys = ON");
        Ok(Session { connection, file: file.to_path_buf(), read_only })
    }

    /// What SQLite calls itself, which is what Test Connection reports.
    pub fn version(&self) -> String {
        format!("SQLite {}", rusqlite::version())
    }

    /// A handle that another thread can use to stop a statement that is running.
    ///
    /// SQLite's own `sqlite3_interrupt`. The same shape as PostgreSQL's cancellation and for the same
    /// reason: a flag this thread would have to notice is no use while it is inside the engine.
    pub fn interrupt(&self) -> rusqlite::InterruptHandle {
        self.connection.get_interrupt_handle()
    }

    /// Run one statement, with its values bound as parameters.
    ///
    /// One function rather than the two PostgreSQL needs, because `rusqlite` prepares everything and
    /// there is no simple-query shape to be cheaper than it. `limit` is asked for as `limit + 1` rows
    /// and cut back, which is what makes `1-200 of 200+` honest.
    pub fn run(&mut self, statement: &str, values: &[Value], limit: usize) -> Answer<Rows> {
        let started = Instant::now();
        let mut prepared = self.connection.prepare(statement).map_err(said)?;
        let count = prepared.column_count();
        // The name and the declared type together. A column of an expression has no declared type —
        // SQLite has none to give — and the empty string is the honest answer for it rather than a
        // guess made from the first row's value, which would be a different answer per page.
        let described: Vec<(String, String)> = prepared
            .columns()
            .iter()
            .map(|column| (column.name().to_owned(), column.decl_type().unwrap_or_default().to_owned()))
            .collect();
        let bound: Vec<rusqlite::types::Value> = values.iter().map(to_sqlite).collect();
        let mut out = Rows::default();
        if count == 0 {
            // A statement that returns nothing: `execute` is what reports how many rows it changed.
            let affected = prepared
                .execute(rusqlite::params_from_iter(bound.iter()))
                .map_err(said)?;
            out.affected = Some(affected as u64);
            out.tag = format!("{affected} rows");
            out.elapsed = started.elapsed();
            return Ok(out);
        }
        // The declared types, which is what SQLite has instead of a type per value. A column of an
        // expression has none, and `` is the honest answer for it.
        out.columns = described
            .iter()
            .map(|(name, declared)| Column::new(name.clone(), declared.clone()))
            .collect();
        let mut rows = prepared.query(rusqlite::params_from_iter(bound.iter())).map_err(said)?;
        while let Some(row) = rows.next().map_err(said)? {
            if out.rows.len() >= limit {
                out.more = true;
                break;
            }
            out.rows.push((0..count).map(|index| from_sqlite(row.get_ref(index))).collect());
        }
        out.tag = format!("{} rows", out.rows.len());
        out.elapsed = started.elapsed();
        Ok(out)
    }

    /// Run several statements as one transaction, stopping and undoing everything at the first
    /// failure. This is what Submit does.
    ///
    /// `check` is applied **before the commit**, which is the whole reason it is an argument rather
    /// than something the caller does afterwards: a postcondition tested after `COMMIT` is not a
    /// postcondition, it is a report about something that has already happened. `Transaction` rolls
    /// back when it is dropped without committing, so returning early here undoes everything.
    pub fn in_one_transaction(
        &mut self,
        work: &[(String, Vec<Value>)],
        check: impl Fn(&[u64]) -> Answer<()>,
    ) -> Answer<Vec<u64>> {
        let transaction = self.connection.transaction().map_err(said)?;
        let mut affected = Vec::with_capacity(work.len());
        for (statement, values) in work {
            let bound: Vec<rusqlite::types::Value> = values.iter().map(to_sqlite).collect();
            let count = transaction
                .execute(statement, rusqlite::params_from_iter(bound.iter()))
                .map_err(said)?;
            affected.push(count as u64);
        }
        check(&affected)?;
        transaction.commit().map_err(said)?;
        Ok(affected)
    }

    /// The one schema a SQLite file has.
    ///
    /// Answered as a list of one rather than as nothing, so the tree has the same shape for both
    /// engines and `components::database::tree` never asks which it is drawing.
    pub fn schemas(&self) -> Vec<String> {
        vec!["main".to_owned()]
    }

    /// Everything in the file that the tree draws a row for.
    pub fn items(&mut self) -> Answer<Vec<Item>> {
        let rows = self.run(
            "select name, type from sqlite_master \
             where type in ('table','view','index') and name not like 'sqlite_%' \
             order by type, name",
            &[],
            usize::MAX,
        )?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|row| {
                let name = row.first()?.text()?.to_owned();
                let kind = match row.get(1)?.text()? {
                    "table" => Kind::Table,
                    "view" => Kind::View,
                    "index" => Kind::Index,
                    _ => return None,
                };
                Some(Item { name, kind })
            })
            .collect())
    }

    /// One table's columns, with a key that may be SQLite's own.
    pub fn table(&mut self, name: &str) -> Answer<Table> {
        // `PRAGMA table_info` takes no parameter, so the name is quoted rather than bound — which is
        // the one place in this crate that is true, and the reason `catalog::quoted` exists.
        let rows = self.run(
            &format!("PRAGMA table_info({})", crate::catalog::quoted(name, '"')),
            &[],
            usize::MAX,
        )?;
        if rows.rows.is_empty() {
            return Err(Failure::said(format!("`{name}` has no columns, or is not there any more.")));
        }
        let mut table = Table { schema: String::new(), name: name.to_owned(), ..Table::default() };
        let mut key: Vec<(i64, String)> = Vec::new();
        for row in &rows.rows {
            let column_name = row.get(1).and_then(Value::text).unwrap_or_default().to_owned();
            let declared = row.get(2).and_then(Value::text).unwrap_or_default().to_owned();
            let not_null = row.get(3).and_then(Value::text) == Some("1");
            let at: i64 = row.get(5).and_then(Value::text).and_then(|at| at.parse().ok()).unwrap_or(0);
            let mut column = Column::new(&column_name, declared);
            column.not_null = not_null;
            column.in_key = at > 0;
            if at > 0 {
                key.push((at, column_name));
            }
            table.columns.push(column);
        }
        key.sort_by_key(|(at, _)| *at);
        table.key = key.into_iter().map(|(_, name)| name).collect();
        if table.key.is_empty() {
            // **A table with no declared key is still addressable here**, which is a real difference
            // from PostgreSQL rather than a convenience: SQLite gives every ordinary table a `rowid`,
            // and a hidden column that names exactly one row is exactly what the editing rule asks
            // for. A `WITHOUT ROWID` table has none, and so does one that has shadowed every alias,
            // and both are then read-only for the same reason a PostgreSQL table with no key is.
            if let Some(alias) = self.an_unshadowed_rowid(name, &table)? {
                table.key = vec![alias.clone()];
                let mut column = Column::new(&alias, "INTEGER");
                column.in_key = true;
                column.not_null = true;
                table.columns.insert(0, column);
            }
        }
        Ok(table)
    }

    /// Which of SQLite's three names for its implicit key this table has not shadowed, if any.
    ///
    /// Two things have to be true of the answer, and both are checked: the table must not declare a
    /// column of that name — because then the name means *that* column, which need not be unique —
    /// and selecting it must work, because a `WITHOUT ROWID` table has no implicit key at all and
    /// refuses every one of the three.
    fn an_unshadowed_rowid(&mut self, name: &str, table: &Table) -> Answer<Option<String>> {
        for alias in ROWID_ALIASES {
            // SQLite identifiers are case-insensitive, so a column called `ROWID` shadows `rowid`.
            let shadowed = table
                .columns
                .iter()
                .any(|column| column.name.eq_ignore_ascii_case(alias));
            if shadowed {
                continue;
            }
            let statement =
                format!("select {alias} from {} limit 1", crate::catalog::quoted(name, '"'));
            if self.run(&statement, &[], 1).is_ok() {
                return Ok(Some((*alias).to_owned()));
            }
        }
        Ok(None)
    }

    /// The `CREATE` statement, which SQLite keeps verbatim.
    ///
    /// The one place either engine can hand back **what was actually typed**, which is why this needs
    /// no note saying it was composed — unlike the PostgreSQL one.
    pub fn ddl(&mut self, name: &str) -> Answer<String> {
        let rows = self.run(
            "select sql from sqlite_master where name = ?1",
            &[Value::typed(name)],
            usize::MAX,
        )?;
        rows.rows
            .first()
            .and_then(|row| row.first())
            .and_then(Value::text)
            .map(str::to_owned)
            .ok_or_else(|| Failure::said(format!("`{name}` is not in this file's schema.")))
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

/// A cell going out.
fn to_sqlite(value: &Value) -> rusqlite::types::Value {
    match value {
        Value::Null => rusqlite::types::Value::Null,
        // Text, always, and never a guess at a number: SQLite's own type affinity converts a string
        // to the column's declared type on the way in, which is a rule the engine has and this client
        // would only ever get differently.
        Value::Text(text) => rusqlite::types::Value::Text(text.clone()),
        Value::Bytes(bytes) => rusqlite::types::Value::Blob(bytes.clone()),
    }
}

/// A cell coming back.
fn from_sqlite(value: rusqlite::Result<ValueRef<'_>>) -> Value {
    match value {
        Ok(ValueRef::Null) => Value::Null,
        Ok(ValueRef::Integer(number)) => Value::Text(number.to_string()),
        Ok(ValueRef::Real(number)) => Value::Text(number.to_string()),
        Ok(ValueRef::Text(bytes)) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
        Ok(ValueRef::Blob(bytes)) => Value::Bytes(bytes.to_vec()),
        // A column that cannot be read is one cell's problem and not the whole result's.
        Err(_) => Value::Null,
    }
}

/// SQLite's own words, which is what a refusal quotes.
fn said(why: rusqlite::Error) -> Failure {
    let code = match &why {
        rusqlite::Error::SqliteFailure(error, _) => format!("SQLITE_{:?}", error.code),
        _ => String::new(),
    };
    Failure { message: why.to_string(), code, ..Failure::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with something in it, in a folder of this test's own.
    ///
    /// Each test names its own file, which is `git_folder(name)`'s rule in the screenshot tests: a
    /// fixture only one test uses may be written each time, and the name is what keeps them apart.
    fn a_database(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("unluminous-db-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        let file = folder.join("test.db");
        let _ = std::fs::remove_file(&file);
        let connection = Connection::open(&file).expect("a database");
        connection
            .execute_batch(
                "create table member (id integer primary key, name text not null, note text);
                 insert into member (id, name, note) values (1, 'Jason', null), (2, 'Ada', '');
                 create table pairs (left text, right text);
                 insert into pairs values ('a', 'b');
                 create view members as select id, name from member;",
            )
            .expect("a schema");
        file
    }

    #[test]
    fn a_file_that_is_not_there_is_said_rather_than_created() {
        // `rusqlite`'s default flags include SQLITE_OPEN_CREATE, so a mistyped path would otherwise
        // make an empty database and the tree would show a data source with nothing in it.
        let missing = std::env::temp_dir().join("unluminous-db-no-such-file-ever.db");
        let _ = std::fs::remove_file(&missing);
        let refused = Session::open(&missing, false).expect_err("refused");
        assert!(refused.message.contains("there is no file at"), "{refused}");
        assert!(!missing.exists(), "and nothing was created");
    }

    #[test]
    fn the_tree_reads_tables_and_views_and_leaves_sqlites_own_alone() {
        let mut session = Session::open(&a_database("items"), false).expect("opened");
        let items = session.items().expect("items");
        let named: Vec<(&str, Kind)> =
            items.iter().map(|item| (item.name.as_str(), item.kind)).collect();
        assert!(named.contains(&("member", Kind::Table)));
        assert!(named.contains(&("pairs", Kind::Table)));
        assert!(named.contains(&("members", Kind::View)));
        assert!(!named.iter().any(|(name, _)| name.starts_with("sqlite_")));
        assert_eq!(session.schemas(), ["main"]);
    }

    #[test]
    fn a_table_that_declares_its_own_rowid_column_does_not_get_it_as_a_key() {
        // SQLite lets a table declare a real column called `rowid`, and once it does, `rowid` in a
        // statement means that column — which need not be unique. Taking it as the key would produce
        // an `UPDATE … WHERE "rowid" = ?` matching two rows, which is the one thing the editing rule
        // exists to prevent.
        let folder = std::env::temp_dir().join(format!("unluminous-db-shadow-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        let file = folder.join("shadow.db");
        let _ = std::fs::remove_file(&file);
        let connection = Connection::open(&file).expect("a database");
        connection
            .execute_batch(
                "create table shadowed (rowid text, note text);
                 insert into shadowed values ('same', 'one'), ('same', 'two');
                 create table plain (a text);
                 insert into plain values ('x');
                 create table every_alias (rowid text, _rowid_ text, oid text);",
            )
            .expect("a schema");
        let mut session = Session::open(&file, false).expect("opened");

        let shadowed = session.table("shadowed").expect("a table");
        assert_eq!(shadowed.key, ["_rowid_"], "the next alias it has not shadowed");
        assert!(shadowed.can_be_changed());

        // And a table that has shadowed all three has no key at all, and is read only.
        let all = session.table("every_alias").expect("a table");
        assert!(all.key.is_empty(), "{:?}", all.key);
        assert!(!all.can_be_changed());
        assert!(all.why_not_changeable().unwrap().contains("no primary key"));

        // The ordinary case is unchanged.
        assert_eq!(session.table("plain").expect("a table").key, ["rowid"]);
    }

    #[test]
    fn a_declared_key_is_the_key_and_a_table_without_one_falls_back_to_the_rowid() {
        let mut session = Session::open(&a_database("keys"), false).expect("opened");
        let member = session.table("member").expect("a table");
        assert_eq!(member.key, ["id"]);
        assert!(member.can_be_changed());
        assert!(member.columns.iter().any(|column| column.name == "name" && column.not_null));

        // The real difference from PostgreSQL: a table nobody gave a key is still addressable, so it
        // is editable here where the same table would be read-only there.
        let pairs = session.table("pairs").expect("a table");
        assert_eq!(pairs.key, [ROWID]);
        assert!(pairs.can_be_changed());
        assert_eq!(pairs.columns.first().map(|column| column.name.as_str()), Some(ROWID));
    }

    #[test]
    fn null_and_the_empty_string_survive_the_round_trip() {
        // The fault a grid that draws both as nothing would hide, checked at the engine boundary.
        let mut session = Session::open(&a_database("nulls"), false).expect("opened");
        let rows = session.run("select note from member order by id", &[], usize::MAX).expect("rows");
        assert_eq!(rows.rows[0][0], Value::Null);
        assert_eq!(rows.rows[1][0], Value::Text(String::new()));
    }

    #[test]
    fn a_value_with_a_quote_a_newline_and_a_backslash_in_it_is_a_non_event() {
        // The case that is a fault in every implementation that builds SQL by concatenation.
        let mut session = Session::open(&a_database("awkward"), false).expect("opened");
        let awkward = "it's \"quoted\";\nand C:\\dev\\ -- not a comment";
        session
            .run("insert into member (id, name) values (?1, ?2)", &[Value::typed("9"), Value::typed(awkward)], 0)
            .expect("inserted");
        let rows = session
            .run("select name from member where id = ?1", &[Value::typed("9")], usize::MAX)
            .expect("rows");
        assert_eq!(rows.rows[0][0], Value::Text(awkward.to_owned()));
    }

    #[test]
    fn a_limit_asks_for_one_more_than_it_keeps_so_that_more_is_honest() {
        let mut session = Session::open(&a_database("limits"), false).expect("opened");
        let one = session.run("select id from member order by id", &[], 1).expect("rows");
        assert_eq!(one.rows.len(), 1);
        assert!(one.more, "there is another row and nobody counted the rest");
        let all = session.run("select id from member order by id", &[], usize::MAX).expect("rows");
        assert_eq!(all.rows.len(), 2);
        assert!(!all.more);
    }

    #[test]
    fn a_transaction_that_fails_half_way_leaves_nothing_behind() {
        let file = a_database("transaction");
        let mut session = Session::open(&file, false).expect("opened");
        let work = vec![
            ("insert into member (id, name) values (?1, ?2)".to_owned(), vec![Value::typed("3"), Value::typed("Grace")]),
            // The second one breaks the NOT NULL, so the first must not survive either.
            ("insert into member (id, name) values (?1, ?2)".to_owned(), vec![Value::typed("4"), Value::Null]),
        ];
        assert!(session.in_one_transaction(&work, |_| Ok(())).is_err());
        let rows = session.run("select count(*) from member", &[], usize::MAX).expect("rows");
        assert_eq!(rows.rows[0][0], Value::Text("2".to_owned()), "neither insert survived");
    }

    #[test]
    fn a_read_only_source_is_refused_by_sqlite_rather_than_by_unluminous() {
        // The guarantee is the engine's. Unluminous also hides the editing controls, but a statement that
        // got past that check is still refused here.
        let file = a_database("readonly");
        let mut session = Session::open(&file, true).expect("opened");
        assert!(session.run("select count(*) from member", &[], usize::MAX).is_ok());
        let refused = session
            .run("insert into member (id, name) values (99, 'x')", &[], 0)
            .expect_err("refused");
        assert!(refused.message.to_lowercase().contains("readonly"), "{refused}");
    }

    #[test]
    fn the_ddl_is_the_text_that_was_typed() {
        // The one place either engine hands back what was actually written, rather than something
        // composed from a catalogue.
        let mut session = Session::open(&a_database("ddl"), false).expect("opened");
        let ddl = session.ddl("member").expect("ddl");
        assert!(ddl.starts_with("CREATE TABLE member"), "{ddl}");
        assert!(ddl.contains("name text not null"), "verbatim, down to the case: {ddl}");
    }
}
