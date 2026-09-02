//! Connect to a real database and say what is in it.
//!
//! The scripted server in `tests/scripted_server.rs` is evidence about the *protocol*; this is how the
//! PostgreSQL half is checked against a real server, which is a different question and the one the
//! ticket asks. It is a command rather than a test because a test that needs a server running on the
//! machine is a test that fails on the machines that have not got one — the rule
//! `tools/build-git-demo.ps1` already keeps for the Git menu.
//!
//! ```text
//! cargo run -p quill-db --example connect -- "postgres://postgres@localhost:5432/ai" PGPASSWORD
//! cargo run -p quill-db --example connect -- C:\jason\dev\quill\tasks.db
//! ```
//!
//! The second argument is the **name of an environment variable**, never a password: the rule the
//! whole plugin keeps, applied to its own diagnostic tool so that a shell history and a process list
//! never hold one.

use quill_db::source::{Secret, Source};
use quill_db::{Database, Kind};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(url) = arguments.next() else {
        eprintln!("usage: connect <url or sqlite file> [name of the environment variable holding the password]");
        std::process::exit(2);
    };
    let variable = arguments.next();

    let mut source = match Source::parse("check", &url) {
        Ok(source) => source,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    };
    let password = variable.as_ref().and_then(|name| {
        source.secret = Secret::Environment(name.clone());
        std::env::var(name).ok()
    });
    if let Some(name) = &variable {
        println!("password: from the environment variable {name}, {}", match password.is_some() {
            true => "which is set",
            false => "which is NOT set",
        });
    }

    let started = std::time::Instant::now();
    let mut database = match Database::connect(&source, password.as_deref()) {
        Ok(database) => database,
        Err(why) => {
            eprintln!("could not connect: {why}");
            std::process::exit(1);
        }
    };
    println!("connected to {} in {} ms", database.version(), started.elapsed().as_millis());
    println!("encrypted: {}", database.is_encrypted());

    match database.databases() {
        Ok(names) => println!("databases: {}", names.join(", ")),
        Err(why) => println!("databases: {why}"),
    }
    let schemas = database.schemas().unwrap_or_default();
    println!("schemas: {}", schemas.join(", "));

    // The first schema that has anything in it, which on PostgreSQL is `public` and on SQLite is the
    // only one there is.
    for schema in schemas.iter().take(4) {
        let items = match database.items(schema) {
            Ok(items) => items,
            Err(why) => {
                println!("  {schema}: {why}");
                continue;
            }
        };
        let tables: Vec<&str> = items
            .iter()
            .filter(|item| item.kind == Kind::Table)
            .map(|item| item.name.as_str())
            .collect();
        println!("  {schema}: {} items, {} of them tables", items.len(), tables.len());
        let Some(first) = tables.first() else { continue };
        match database.table(schema, first) {
            Ok(table) => {
                println!(
                    "    {first}: {} columns, key [{}], {}",
                    table.columns.len(),
                    table.key.join(", "),
                    match table.can_be_changed() {
                        true => "editable",
                        false => "read-only in the grid",
                    }
                );
                let statement = format!("select * from {} limit 5", table.qualified('"'));
                match database.query(&statement, 5) {
                    Ok(rows) => println!(
                        "    {statement} -> {} rows in {} ms{}",
                        rows.rows.len(),
                        rows.elapsed.as_millis(),
                        match rows.more {
                            true => ", and there are more",
                            false => "",
                        }
                    ),
                    Err(why) => println!("    {statement} -> {why}"),
                }
            }
            Err(why) => println!("    {first}: {why}"),
        }
        break;
    }
    database.close();
}
