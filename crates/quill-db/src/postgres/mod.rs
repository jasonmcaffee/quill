//! Speaking to PostgreSQL, by hand.
//!
//! Four files and nothing else: the frames, the SCRAM exchange, the connection, and the catalogue
//! queries the tree asks. `tasks/task-1777-database-plugin-tdd.md` §2 records why this is written here
//! rather than taken from `tokio-postgres` — a blocking façade over an async client that compiles a
//! runtime into a program which has none, at fifty crates against the two this adds.
//!
//! It is arranged the way `quill-dap` speaks the Debug Adapter Protocol and `quill-chat` speaks
//! server-sent events, down to the tests: a **scripted server** on `127.0.0.1:0` replaying fixed
//! bytes, which is what makes "the whole client, end to end" a unit test with no database in it.

pub mod introspect;
pub mod scram;
pub mod session;
pub mod wire;

pub use session::Session;
