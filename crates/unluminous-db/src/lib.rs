//! Reading and changing a database, with no user interface in it.
//!
//! The sixth crate, and it is arranged the way `unluminous-dap` and `unluminous-chat` are: the wire, the values,
//! the session and the thread. Its tests run with no window, no graphics card and no fonts — and, for
//! the PostgreSQL half, against a **scripted server** on `127.0.0.1:0` replaying fixed bytes, which is
//! `unluminous-dap`'s scripted adapters with a socket instead of a pipe.
//!
//! ## Why the PostgreSQL client is written here
//!
//! `postgres` 0.19 is a blocking façade over `tokio-postgres`: it builds a Tokio runtime and runs the
//! connection on it. Unluminous has no runtime on purpose — `unluminous_git::Worker`, the terminal's reader,
//! `text_search`, `symbol_index`, `unluminous-dap` and `unluminous-chat` are all plain threads with channels,
//! and the workspace `Cargo.toml` says in as many words that "a runtime added for one pane would be a
//! second concurrency model in a program that has one". Shelling out to `psql` is not `unluminous-git`'s
//! situation either: `git` is on this machine because the person uses git, and `psql` is not
//! necessarily on a machine whose *server* is somewhere else, while its output is a report meant for a
//! person rather than a format designed to be parsed.
//!
//! So the protocol is spoken here. Counted rather than guessed, that costs **two crates** — `hmac` and
//! `md-5` — because `sha2`, `base64`, `native-tls` and `getrandom` are already in the tree by another
//! route. `tokio-postgres` is roughly fifty. `tasks/task-1777-database-plugin-tdd.md` §2 is the whole
//! argument.
//!
//! ## Two rules that everything else rests on
//!
//! **Every value arrives as the text the server printed**, so there is one decoder rather than one per
//! type, and the grid shows exactly what `psql` would show. See `value.rs`.
//!
//! **A row can only be changed if it can be addressed** — one table, with a primary key, or SQLite's
//! own `rowid`. There is deliberately no fallback that matches on every column, because that is the
//! fallback which quietly changes two identical rows. See `catalog::Table::can_be_changed` and
//! `edit.rs`.

pub mod catalog;
pub mod edit;
pub mod engine;
pub mod postgres;
pub mod rows;
pub mod source;
pub mod sql;
pub mod sqlite;
pub mod value;
pub mod worker;

pub use catalog::{Item, Kind, Table};
pub use edit::{Change, Pending, Row, Statement};
pub use engine::Database;
pub use rows::{Answer, Failure, Rows};
pub use source::{Engine, Secret, Source, SslMode};
pub use value::{Column, Value};
pub use worker::{Job, Reply, Worker};
