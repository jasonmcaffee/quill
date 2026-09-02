//! The Database plugin, with no window.
//!
//! Every test here drives the provider the way its buttons and its commands do — through
//! `UiProvider::command`, which is the one path a change goes down — against a **real SQLite file**
//! built in a temporary folder. A PostgreSQL server cannot be assumed on the machine running a test;
//! `crates/quill-db/tests/scripted_server.rs` is where the wire protocol is tested, and
//! `cargo run -p quill-db --example connect` is how the real server is.

use std::path::PathBuf;

use quill_db::source::{Secret, Source};
use quill_db::value::Value;

use super::*;
use crate::services::plugin_ui::{Context, UiProvider};

/// A database file with something in it, in a folder of this test's own.
fn a_database(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("quill-database-plugin-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let file = folder.join("test.db");
    let _ = std::fs::remove_file(&file);
    let connection = rusqlite::Connection::open(&file).expect("a database");
    connection
        .execute_batch(
            "create table member (id integer primary key, name text not null, note text);
             insert into member (id, name, note) values (1, 'Jason', null), (2, 'Ada', '');
             create table pairs (left text, right text);
             insert into pairs values ('a', 'b'), ('c', 'd');
             create view members as select id, name from member;",
        )
        .expect("a schema");
    file
}

/// A provider opened on a folder of its own, with one SQLite data source in it.
fn opened(name: &str) -> (DatabaseExplorer, PathBuf) {
    let file = a_database(name);
    let folder = file.parent().expect("a folder").to_path_buf();
    let mut explorer = DatabaseExplorer::new();
    explorer.open(&Context { folder: Some(folder), ..Context::default() }).expect("opened");
    explorer
        .command("add-source", &["test".to_owned(), file.to_string_lossy().into_owned()])
        .expect("added");
    (explorer, file)
}

/// Run a command and take the sentence it answered.
fn run(explorer: &mut DatabaseExplorer, command: &str, arguments: &[&str]) -> String {
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    explorer
        .command(command, &arguments)
        .unwrap_or_else(|why| panic!("{command}: {why}"))
        .message
}

/// Run a command and take the value.
fn value(explorer: &mut DatabaseExplorer, command: &str, arguments: &[&str]) -> serde_json::Value {
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    explorer
        .command(command, &arguments)
        .unwrap_or_else(|why| panic!("{command}: {why}"))
        .value
}

/// Wait for whatever the workers are doing, with a deadline rather than for ever.
fn settle(explorer: &mut DatabaseExplorer) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < until {
        explorer.take_the_replies();
        if !explorer.anything_running() {
            // One more pass, because the answer to the last job may have arrived on this one.
            explorer.take_the_replies();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("something is still running after ten seconds");
}

#[test]
fn nothing_is_connected_until_something_asks() {
    // The laziness `UiProvider::open` describes: opening a pane must not open a socket to somebody's
    // server. For SQLite it is the difference between opening a file at startup and opening it when
    // the tree is first looked at.
    let (mut explorer, _) = opened("lazy");
    assert!(!explorer.is_connected("test"));
    explorer.toggle_source("test");
    assert!(explorer.is_connected("test"));
}

#[test]
fn the_tree_reads_one_level_at_a_time() {
    let (mut explorer, _) = opened("tree");
    explorer.toggle_source("test");
    settle(&mut explorer);
    let loaded = explorer.loaded.get("test").expect("a source");
    assert_eq!(loaded.schemas, ["main"]);
    assert!(loaded.items.is_empty(), "nothing under a schema until the schema is opened");

    explorer.toggle_schema("test", "main");
    settle(&mut explorer);
    let items = explorer.loaded["test"].items["main"].clone();
    assert!(items.iter().any(|item| item.name == "member" && item.kind == quill_db::Kind::Table));
    assert!(items.iter().any(|item| item.name == "members" && item.kind == quill_db::Kind::View));
    assert!(explorer.loaded["test"].columns.is_empty(), "and no columns until a table is opened");

    explorer.toggle_table("test", "main", "member");
    settle(&mut explorer);
    let table = explorer.loaded["test"].columns[&("main".to_owned(), "member".to_owned())].clone();
    assert_eq!(table.key, ["id"]);
    assert!(table.columns.iter().any(|column| column.name == "name" && column.not_null));
}

#[test]
fn a_console_runs_the_statement_under_the_caret() {
    // Which is the whole reason `quill_db::sql::at` exists: a console holds several statements and
    // running the file is never what was meant.
    let (mut explorer, _) = opened("console");
    let page = value(&mut explorer, "console", &["test"]);
    let id = page["page"].as_u64().expect("a page");
    if let Some(Page { sheet: Sheet::Console(console), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        console.text = "select 1;\nselect name from member order by id;".to_owned();
        // The caret on the second line.
        console.caret = 12;
    }
    explorer.execute(id).expect("sent");
    settle(&mut explorer);
    let result = value(&mut explorer, "result", &[]);
    let rows = &result["result"]["rows"];
    assert_eq!(rows[0][0], serde_json::json!("Jason"), "the second statement ran, not the first");
    assert_eq!(result["result"]["count"], serde_json::json!(2));
}

#[test]
fn a_grid_opens_on_a_table_and_reads_its_rows() {
    let (mut explorer, _) = opened("grid");
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    run(&mut explorer, "open", &["main.member"]);
    settle(&mut explorer);
    let page = value(&mut explorer, "result", &[]);
    assert_eq!(page["kind"], serde_json::json!("grid"));
    assert_eq!(page["table"], serde_json::json!("member"));
    assert_eq!(page["key"], serde_json::json!(["id"]));
    assert_eq!(page["editable"], serde_json::json!(true));
    assert_eq!(page["rows"]["count"], serde_json::json!(2));
}

#[test]
fn a_table_with_no_key_is_editable_here_because_sqlite_gives_it_a_rowid() {
    // The real difference from PostgreSQL, and the one place a grid is editable that its own schema
    // did not ask for: SQLite gives every ordinary table a `rowid`, which names exactly one row.
    let (mut explorer, _) = opened("rowid");
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    run(&mut explorer, "open", &["main.pairs"]);
    settle(&mut explorer);
    let page = value(&mut explorer, "result", &[]);
    assert_eq!(page["key"], serde_json::json!(["rowid"]));
    assert_eq!(page["editable"], serde_json::json!(true));
    assert_eq!(page["why_not_editable"], serde_json::Value::Null);
}

#[test]
fn a_view_is_read_only_and_says_why() {
    // A view's rows belong to the tables underneath it, so they are read here and changed there. The
    // controls are absent rather than dimmed, and the sentence is what goes where they were.
    let (mut explorer, _) = opened("view");
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    // The items have to be read before the kind of a name is known.
    run(&mut explorer, "tables", &["main"]);
    settle(&mut explorer);
    run(&mut explorer, "open", &["main.members"]);
    settle(&mut explorer);
    let page = value(&mut explorer, "result", &[]);
    assert_eq!(page["editable"], serde_json::json!(false));
    let why = page["why_not_editable"].as_str().unwrap_or_default();
    assert!(why.contains("view"), "{why}");
    // And an agent asking to change one is refused with the same sentence rather than quietly
    // recording a change that could never be written.
    let refused = explorer
        .command("set", &["1".to_owned(), "name".to_owned(), "Ada".to_owned()])
        .expect_err("refused");
    assert!(refused.contains("view"), "{refused}");
}

#[test]
fn an_edit_is_pending_until_it_is_submitted_and_then_the_file_really_changes() {
    let (mut explorer, file) = opened("submit");
    // A data source is read only until somebody says otherwise, so this is also what proves that
    // switch does something.
    run(&mut explorer, "read-only", &["test", "off"]);
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    run(&mut explorer, "open", &["main.member"]);
    settle(&mut explorer);

    run(&mut explorer, "set", &["1", "name", "Grace"]);
    let pending = value(&mut explorer, "pending", &[]);
    assert_eq!(pending.as_array().map(Vec::len), Some(1));
    // The preview is the **statement**, not a summary of one, and the value is bound rather than
    // pasted in.
    let sql = pending[0]["sql"].as_str().unwrap_or_default();
    assert!(sql.starts_with("UPDATE \"member\" SET \"name\" = ?1 WHERE \"id\" = ?2"), "{sql}");
    assert!(!sql.contains("Grace"), "the value is not in the statement");
    assert_eq!(pending[0]["values"][0], serde_json::json!("Grace"));

    run(&mut explorer, "submit", &[]);
    settle(&mut explorer);
    // Read it back through a connection of its own, so this is the file rather than a cache.
    let connection = rusqlite::Connection::open(&file).expect("opened");
    let name: String = connection
        .query_row("select name from member where id = 1", [], |row| row.get(0))
        .expect("a row");
    assert_eq!(name, "Grace");
}

#[test]
fn a_read_only_data_source_refuses_a_write_and_the_refusal_names_the_switch() {
    let (mut explorer, _) = opened("read-only");
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    let page = value(&mut explorer, "console", &["test"]);
    let id = page["page"].as_u64().expect("a page");
    if let Some(Page { sheet: Sheet::Console(console), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        console.text = "update member set name = 'x'".to_owned();
        console.caret = 0;
    }
    let refused = explorer.execute(id).expect_err("refused");
    assert!(refused.contains("read only"), "{refused}");
    assert!(refused.contains("UPDATE"), "and it names the statement: {refused}");
}

#[test]
fn a_statement_that_changes_rows_is_confirmed_once_before_it_is_sent() {
    // A console is where somebody types `delete from member` meaning to type a `where` clause after
    // it. One dialog is cheaper than the row that is not there any more.
    let (mut explorer, _) = opened("confirm");
    run(&mut explorer, "read-only", &["test", "off"]);
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    let page = value(&mut explorer, "console", &["test"]);
    let id = page["page"].as_u64().expect("a page");
    if let Some(Page { sheet: Sheet::Console(console), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        console.text = "delete from member".to_owned();
        console.caret = 0;
    }
    let said = explorer.execute(id).expect("asked");
    assert!(said.contains("confirm"), "{said}");
    assert!(matches!(explorer.modal, Some(Modal::Confirm { .. })), "{:?}", explorer.modal);
    // A `select` is not asked about, because asking about every statement is how a confirmation stops
    // being read.
    if let Some(Page { sheet: Sheet::Console(console), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        console.text = "select 1".to_owned();
    }
    explorer.modal = None;
    explorer.execute(id).expect("sent");
    assert!(explorer.modal.is_none());
}

#[test]
fn the_view_answers_the_same_numbers_the_pane_is_drawing() {
    // Quill's rule: a pane drawn with `egui` is invisible to a test and to an agent unless it can be
    // read as data, and a screenshot is not an answer to "how many tables are there".
    let (mut explorer, _) = opened("view-data");
    // Opened the way a person opens it — the row in the tree — so that what `view` answers and what
    // the tree draws are read from the same state rather than from two.
    explorer.toggle_source("test");
    settle(&mut explorer);
    explorer.toggle_schema("test", "main");
    settle(&mut explorer);
    explorer.toggle_folder("test", "main", "tables");
    let view = explorer.view();
    assert_eq!(view["sources"][0]["name"], serde_json::json!("test"));
    assert_eq!(view["sources"][0]["connected"], serde_json::json!(true));
    assert!(view["sources"][0]["version"].as_str().unwrap_or_default().starts_with("SQLite"));
    // What the tree would draw, as data: the same lists `components::database::tree::lines` reads.
    let drawn = crate::components::database::tree::lines(&explorer);
    let items = explorer.loaded["test"].items["main"].len();
    assert_eq!(view["tree"][0]["items"][0]["items"].as_array().map(Vec::len), Some(items));
    assert!(drawn.len() > items, "a row per item, plus the source, the schema and the folders");
}

#[test]
fn where_a_password_is_can_be_said_on_the_command_line_and_the_password_itself_cannot() {
    // The gap the first real run against PostgreSQL found: `add-source` took a URL and there was no
    // way at all to say *where* a password is except through the dialog, so an agent could add a data
    // source it could never connect.
    let (mut explorer, _) = opened("password-command");
    run(&mut explorer, "password", &["test", "env", "QUILL_DB_TEST"]);
    assert_eq!(
        explorer.configuration.source("test").map(|source| source.secret.clone()),
        Some(Secret::Environment("QUILL_DB_TEST".to_owned()))
    );
    run(&mut explorer, "password", &["test", "none"]);
    assert_eq!(explorer.configuration.source("test").map(|source| source.secret.clone()), Some(Secret::None));

    // And `add-source` takes the variable as an optional third word, so adding a source and saying
    // where its password is are one command. A path is not mistaken for a variable name.
    explorer
        .command(
            "add-source",
            &["remote".to_owned(), "postgres://me@example.com/db".to_owned(), "QUILL_DB_REMOTE".to_owned()],
        )
        .expect("added");
    assert_eq!(
        explorer.configuration.source("remote").map(|source| source.secret.clone()),
        Some(Secret::Environment("QUILL_DB_REMOTE".to_owned()))
    );
    explorer
        .command("add-source", &["file".to_owned(), r"C:\tmp\notes.db".to_owned()])
        .expect("added");
    assert_eq!(
        explorer.configuration.source("file").map(|source| source.database.clone()),
        Some(r"C:\tmp\notes.db".to_owned()),
        "a path is a path, not a variable name"
    );

    // There is deliberately no way to give Quill the password itself: it would be in a shell history,
    // a process list and a log.
    let refused = explorer
        .command("password", &["test".to_owned(), "hunter2".to_owned()])
        .expect_err("refused");
    assert!(refused.contains("not somewhere a password lives"), "{refused}");
}

#[test]
fn a_password_is_never_written_down_and_never_answered_back() {
    // The rule the whole plugin keeps, checked at the two places it could leak: the file, and what an
    // agent can read.
    let (mut explorer, _) = opened("secrets");
    explorer
        .command("add-source", &["remote".to_owned(), "postgres://me@example.com:5432/db".to_owned()])
        .expect("added");
    if let Some(source) = explorer.configuration.source_mut("remote") {
        source.secret = Secret::Typed("hunter2".to_owned());
    }
    explorer.write_the_configuration().expect("written");
    let folder = explorer.folder.clone().expect("a folder");
    let text = std::fs::read_to_string(folder.join(Configuration::FILE)).expect("the file");
    assert!(!text.contains("hunter2"), "{text}");
    let listed = serde_json::to_string(&explorer.view()).expect("json");
    assert!(!listed.contains("hunter2"), "and an agent cannot read it back either");
    assert!(listed.contains("typed, until this window closes"));
}

#[test]
fn every_command_answers_or_refuses_with_a_sentence_naming_what_it_takes() {
    // `task-1704`'s rule about proportionate replies, and `task-1699`'s about an agent's first guess:
    // a refusal that does not say what the command takes is a refusal an agent cannot recover from.
    let (mut explorer, _) = opened("refusals");
    let refused = explorer.command("nothing-like-this", &[]).expect_err("refused");
    assert!(refused.contains("query"), "the refusal lists the commands: {refused}");

    for (command, arguments, expected) in [
        ("add-source", vec!["justaname"], "name and a URL"),
        ("connect", vec!["nope"], "no data source called"),
        ("set", vec![], "row number"),
        ("open", vec![], "schema.table"),
        ("password", vec!["test", "somewhere"], "not somewhere a password lives"),
        ("read-only", vec!["test"], "`on` or `off`"),
        ("confirm", vec!["maybe"], "`on` or `off`"),
    ] {
        let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
        let refused = explorer.command(command, &arguments).expect_err(command);
        assert!(refused.contains(expected), "{command}: {refused}");
    }

    // `add-source` with nothing said is not a refusal: it opens the dialog, because the menu entry
    // carries no arguments and the entry and the command are one path.
    assert!(explorer.command("add-source", &[]).is_ok());
    assert!(matches!(explorer.modal, Some(Modal::Source(_))), "{:?}", explorer.modal);
    explorer.modal = None;

    // And every command in the list is answered by `run`, so `plugins show database` cannot list one
    // that does not exist.
    for (name, summary) in commands::LIST {
        assert!(!summary.is_empty(), "{name} says nothing about itself");
        let answered = explorer.command(name, &[]);
        if let Err(why) = &answered {
            assert!(
                !why.contains("is not one of the Database plugin's commands"),
                "{name} is listed and not answered"
            );
        }
    }
}

#[test]
fn a_grids_statement_asks_for_one_more_row_than_it_keeps() {
    // Which is what makes `1-200 of 200+` honest: nobody counted the rest.
    let grid = Grid {
        source: "test".to_owned(),
        table: quill_db::Table {
            schema: String::new(),
            name: "member".to_owned(),
            columns: Vec::new(),
            key: vec!["id".to_owned()],
        },
        kind: quill_db::Kind::Table,
        where_clause: "name like 'A%'".to_owned(),
        order_by: "name desc".to_owned(),
        at: 2,
        rows: quill_db::Rows::default(),
        pending: quill_db::Pending::default(),
        failure: None,
        running: None,
        chosen: None,
        editing: None,
    };
    let statement = select_for(&grid, quill_db::Engine::Sqlite, 200, 2);
    assert_eq!(
        statement,
        "select * from \"member\" where name like 'A%' order by name desc limit 201 offset 400"
    );
    // A SQLite table addressed by its `rowid` has to ask for it by name: it is not in `select *`.
    let by_rowid = Grid {
        table: quill_db::Table { key: vec!["rowid".to_owned()], ..grid.table.clone() },
        where_clause: String::new(),
        order_by: String::new(),
        ..grid
    };
    let statement = select_for(&by_rowid, quill_db::Engine::Sqlite, 10, 0);
    assert_eq!(statement, "select rowid, * from \"member\" limit 11 offset 0");
    // And PostgreSQL's `select *` is enough, because a declared key is a column.
    let statement = select_for(&by_rowid, quill_db::Engine::Postgres, 10, 0);
    assert_eq!(statement, "select * from \"member\" limit 11 offset 0");
}

#[test]
fn a_cell_shows_what_is_pending_on_it_rather_than_what_was_read() {
    let mut rows = quill_db::Rows::default();
    rows.columns = vec![quill_db::Column::new("id", "int"), quill_db::Column::new("name", "text")];
    rows.rows = vec![vec![Value::typed("1"), Value::typed("Jason")]];
    let mut grid = Grid {
        source: "test".to_owned(),
        table: quill_db::Table {
            schema: String::new(),
            name: "member".to_owned(),
            columns: rows.columns.clone(),
            key: vec!["id".to_owned()],
        },
        kind: quill_db::Kind::Table,
        where_clause: String::new(),
        order_by: String::new(),
        at: 0,
        rows,
        pending: quill_db::Pending::default(),
        failure: None,
        running: None,
        chosen: None,
        editing: None,
    };
    assert_eq!(grid.cell(0, 1), (Value::typed("Jason"), false));
    let row = grid.row_of(0).expect("a row");
    assert_eq!(row, quill_db::Row::Keyed(vec!["1".to_owned()]));
    grid.pending.set(row, "name", Value::typed("Grace"));
    assert_eq!(grid.cell(0, 1), (Value::typed("Grace"), true), "and it is marked as pending");
    assert_eq!(grid.cell(0, 0), (Value::typed("1"), false), "the cell beside it is untouched");
}

#[test]
fn removing_a_data_source_takes_its_pages_with_it() {
    let (mut explorer, _) = opened("removing");
    run(&mut explorer, "connect", &["test"]);
    settle(&mut explorer);
    run(&mut explorer, "console", &["test"]);
    assert_eq!(explorer.pages.len(), 1);
    run(&mut explorer, "remove-source", &["test"]);
    assert!(explorer.pages.is_empty(), "a page pointed at nothing is a page that cannot be used");
    assert!(!explorer.is_connected("test"));
    assert!(explorer.sources().is_empty());
}

#[test]
fn a_source_that_cannot_be_opened_says_so_on_its_own_row_rather_than_silently() {
    let (mut explorer, _) = opened("missing");
    explorer
        .command("add-source", &["gone".to_owned(), r"C:\no\such\file\anywhere.db".to_owned()])
        .expect("added");
    explorer.toggle_source("gone");
    let said = explorer.loaded["gone"].problem.clone().unwrap_or_default();
    assert!(said.contains("there is no file at"), "{said}");
    // And the tree draws that sentence where the schemas would have been.
    let drawn = crate::components::database::tree::lines(&explorer);
    assert!(
        drawn.iter().any(|line| matches!(&line.what, crate::components::database::tree::What::Problem { .. })),
        "the tree shows the problem"
    );
}

#[test]
fn a_url_a_person_could_type_is_what_comes_back_out() {
    let (mut explorer, _) = opened("urls");
    explorer
        .command("add-source", &["ai".to_owned(), "postgres://postgres@localhost:5432/ai".to_owned()])
        .expect("added");
    let sources = value(&mut explorer, "sources", &[]);
    let url = sources
        .as_array()
        .and_then(|sources| sources.iter().find(|source| source["name"] == serde_json::json!("ai")))
        .and_then(|source| source["url"].as_str())
        .unwrap_or_default();
    assert_eq!(url, "postgres://postgres@localhost:5432/ai?sslmode=prefer");
    assert_eq!(Source::parse("ai", url).expect("read back").database, "ai");
}
