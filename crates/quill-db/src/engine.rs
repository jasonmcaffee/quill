//! The two engines behind one value.
//!
//! An `enum` rather than a trait object, which is `dock::Panel`'s own argument made again: there are
//! two, and a third is a variant the compiler then names every place that has to answer for. Nothing
//! above this line asks which engine it is holding — `components::database::tree` draws
//! `catalog::Item`s and the grid draws `Rows`, and neither has heard of a `RowDescription`.

use std::path::Path;

use crate::catalog::{Item, Kind, Table};
use crate::postgres;
use crate::rows::{Answer, Failure, Rows};
use crate::source::{Engine, Source};
use crate::sqlite;
use crate::value::Value;

/// An open connection to one data source.
#[derive(Debug)]
pub enum Database {
    Postgres(Box<postgres::Session>),
    Sqlite(Box<sqlite::Session>),
}

/// What can stop a statement that is running, from another thread.
///
/// Held separately from the connection because that is the whole point: the thread running the
/// statement is inside the engine and cannot look at a flag, so the thing that stops it has to be
/// reachable from somewhere else. PostgreSQL opens a second connection; SQLite calls
/// `sqlite3_interrupt`.
pub enum Stopper {
    Postgres { host: String, port: u16, key: Option<(u32, u32)>, ssl: crate::source::SslMode },
    Sqlite(rusqlite::InterruptHandle),
}

impl std::fmt::Debug for Stopper {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stopper::Postgres { host, port, .. } => write!(out, "Stopper::Postgres({host}:{port})"),
            Stopper::Sqlite(_) => write!(out, "Stopper::Sqlite"),
        }
    }
}

impl Database {
    /// Open a connection.
    ///
    /// `password` is fetched by the caller at this moment and dropped as soon as this returns.
    pub fn connect(source: &Source, password: Option<&str>) -> Answer<Database> {
        match source.engine {
            Engine::Postgres => {
                Ok(Database::Postgres(Box::new(postgres::Session::connect(source, password)?)))
            }
            Engine::Sqlite => Ok(Database::Sqlite(Box::new(sqlite::Session::open(
                Path::new(&source.database),
                source.read_only,
            )?))),
        }
    }

    /// What the server calls itself — the string Test Connection reports, in the server's own words.
    pub fn version(&self) -> String {
        match self {
            Database::Postgres(session) => match session.version().is_empty() {
                true => "PostgreSQL".to_owned(),
                false => format!("PostgreSQL {}", session.version()),
            },
            Database::Sqlite(session) => session.version(),
        }
    }

    pub fn engine(&self) -> Engine {
        match self {
            Database::Postgres(_) => Engine::Postgres,
            Database::Sqlite(_) => Engine::Sqlite,
        }
    }

    /// True when the connection itself is encrypted. Always false for SQLite, which is a file.
    pub fn is_encrypted(&self) -> bool {
        match self {
            Database::Postgres(session) => session.is_encrypted(),
            Database::Sqlite(_) => false,
        }
    }

    /// The databases on this server, for the top level of the tree.
    ///
    /// One for SQLite — the file — because the tree has one shape and asking which engine it is
    /// drawing is exactly what this enum exists to avoid.
    pub fn databases(&mut self) -> Answer<Vec<String>> {
        match self {
            Database::Postgres(session) => postgres::introspect::databases(session),
            Database::Sqlite(session) => Ok(vec![session
                .file()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "database".to_owned())]),
        }
    }

    pub fn schemas(&mut self) -> Answer<Vec<String>> {
        match self {
            Database::Postgres(session) => postgres::introspect::schemas(session),
            Database::Sqlite(session) => Ok(session.schemas()),
        }
    }

    /// Everything in one schema, including its routines.
    pub fn items(&mut self, schema: &str) -> Answer<Vec<Item>> {
        match self {
            Database::Postgres(session) => {
                let mut items = postgres::introspect::items(session, schema)?;
                items.extend(postgres::introspect::routines(session, schema)?);
                Ok(items)
            }
            Database::Sqlite(session) => session.items(),
        }
    }

    pub fn table(&mut self, schema: &str, name: &str) -> Answer<Table> {
        match self {
            Database::Postgres(session) => postgres::introspect::table(session, schema, name),
            Database::Sqlite(session) => session.table(name),
        }
    }

    pub fn ddl(&mut self, schema: &str, name: &str, kind: Kind) -> Answer<String> {
        match self {
            Database::Postgres(session) => postgres::introspect::ddl(session, schema, name, kind),
            Database::Sqlite(session) => session.ddl(name),
        }
    }

    /// Run a statement somebody typed, with nothing bound.
    pub fn query(&mut self, statement: &str, limit: usize) -> Answer<Rows> {
        match self {
            Database::Postgres(session) => session.simple(statement, limit),
            Database::Sqlite(session) => session.run(statement, &[], limit),
        }
    }

    /// Run a statement Quill composed, with its values bound as parameters.
    pub fn run(&mut self, statement: &str, values: &[Value], limit: usize) -> Answer<Rows> {
        match self {
            Database::Postgres(session) => session.extended(statement, values, limit),
            Database::Sqlite(session) => session.run(statement, values, limit),
        }
    }

    /// Write a list of statements as one transaction, or write none of them.
    ///
    /// **The affected count of each is checked.** An `UPDATE` that reports zero rows means the row
    /// moved underneath — somebody else changed or deleted it since the grid was filled — and
    /// reporting that as a success is how an edit is silently lost. It rolls the whole thing back and
    /// says which statement it was.
    pub fn write(&mut self, work: &[(String, Vec<Value>)]) -> Answer<Vec<u64>> {
        match self {
            Database::Sqlite(session) => {
                let affected = session.in_one_transaction(work)?;
                check(&affected)?;
                Ok(affected)
            }
            Database::Postgres(session) => {
                session.simple("BEGIN", usize::MAX)?;
                let mut affected = Vec::with_capacity(work.len());
                for (statement, values) in work {
                    match session.extended(statement, values, 0) {
                        Ok(rows) => affected.push(rows.affected.unwrap_or_default()),
                        Err(why) => {
                            let _ = session.simple("ROLLBACK", usize::MAX);
                            return Err(why);
                        }
                    }
                }
                if let Err(why) = check(&affected) {
                    let _ = session.simple("ROLLBACK", usize::MAX);
                    return Err(why);
                }
                session.simple("COMMIT", usize::MAX)?;
                Ok(affected)
            }
        }
    }

    /// Set the schema a console's unqualified names are resolved in.
    ///
    /// PostgreSQL's `search_path`, and nothing at all for SQLite, which has one schema. It is a
    /// statement rather than a connection parameter because the schema switcher changes it while the
    /// connection is open.
    pub fn use_schema(&mut self, schema: &str) -> Answer<()> {
        match self {
            Database::Postgres(session) => {
                // The schema name cannot be a parameter — nothing that is part of the statement's own
                // grammar can be — so it is quoted, which is what `catalog::quoted` is for.
                let statement =
                    format!("SET search_path TO {}", crate::catalog::quoted(schema, '"'));
                session.simple(&statement, usize::MAX).map(|_| ())
            }
            Database::Sqlite(_) => Ok(()),
        }
    }

    /// What another thread can stop this connection with.
    pub fn stopper(&self) -> Stopper {
        match self {
            Database::Postgres(session) => session.stopper(),
            Database::Sqlite(session) => Stopper::Sqlite(session.interrupt()),
        }
    }

    pub fn close(&mut self) {
        if let Database::Postgres(session) = self {
            session.close();
        }
    }
}

impl Stopper {
    /// Ask the engine to stop what it is doing. Never waits for an answer: PostgreSQL sends none, and
    /// SQLite's interrupt returns at once.
    pub fn stop(&self) -> Answer<()> {
        match self {
            Stopper::Postgres { host, port, key, ssl } => {
                postgres::session::cancel(host, *port, *key, *ssl)
            }
            Stopper::Sqlite(handle) => {
                handle.interrupt();
                Ok(())
            }
        }
    }
}

/// An affected count of zero from a statement that named one row.
fn check(affected: &[u64]) -> Answer<()> {
    match affected.iter().position(|count| *count == 0) {
        Some(at) => Err(Failure::said(format!(
            "statement {} changed no rows, which means that row is not there any more — somebody \
             else has changed or deleted it since this grid was filled. Nothing was written.",
            at + 1
        ))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_statement_that_changed_nothing_stops_the_whole_write() {
        // The fault this exists to prevent: a row somebody else deleted, an `UPDATE` that matches
        // nothing, and a grid that reports the edit as saved.
        assert!(check(&[1, 1, 1]).is_ok());
        let refused = check(&[1, 0, 1]).expect_err("refused");
        assert!(refused.message.contains("statement 2"), "{refused}");
        assert!(refused.message.contains("Nothing was written"), "{refused}");
    }
}
