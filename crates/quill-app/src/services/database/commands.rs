//! What the menu entries, the buttons in the pane and `quill-cli plugins run database …` all call.
//!
//! **One path**, which is `QuillApp::run_action`'s rule for the menus and `run_cli`'s for the command
//! line, kept here for a plugin: a thing done by hand and the same thing done by an agent are the same
//! call rather than two that agree today.
//!
//! Two of these deliberately do not wait. `query` starts a statement and answers with the ticket it
//! will come back under, because `UiProvider::command` runs inside a frame and a command that blocked
//! would stop the window drawing for the length of a query — the sentence `quill_git::Worker` exists
//! for, and the shape `run start` and `run output` already have. `state` says when it has finished and
//! `result` is where the rows are. Each summary says so, because an agent that does not know will ask
//! for the result too early exactly once.

use quill_db::source::{Secret, Source};
use quill_db::value::Value;

use crate::services::database::{DatabaseExplorer, Page, Sheet, SourceForm};
use crate::services::plugin_ui::{Answer, UiProvider};

/// Every command, with the one line `plugins show database` prints for each.
pub const LIST: &[(&'static str, &'static str)] = &[
    ("open-pane", "Show the data source tree."),
    ("open-tab", "Show the workspace: the consoles and the row editors."),
    ("sources", "Every data source: where it points, whether it is connected, and where its password is. Never the password."),
    ("add-source", "Add one. Takes a name and a `postgres://…` URL, or a name and the path of a SQLite file, and optionally the NAME of an environment variable holding the password."),
    ("remove-source", "Take one away, by name."),
    ("connect", "Open the connection to a data source, by name."),
    ("disconnect", "Close it."),
    ("use", "Which data source the tree and a new console are pointed at."),
    ("schemas", "The schemas of a data source."),
    ("tables", "What is in a schema: tables, views, routines and sequences."),
    ("columns", "One table's columns, its types and its primary key."),
    ("ddl", "The `CREATE` statement for a table or a view."),
    ("open", "Open a grid on a table's rows. Takes `schema.table` or just `table`."),
    ("console", "Open a query console on a data source."),
    ("query", "Run a statement. **Does not wait**; `state` says when it has finished and `result` has the rows."),
    ("state", "Whether the current page is still running, and what it last said."),
    ("result", "The columns and rows of the current page, bounded by the row limit."),
    ("stop", "Stop what the current page's data source is doing."),
    ("reload", "Read the current grid again."),
    ("page", "Which page of a grid: `next`, `previous`, `first`, or a number from 1."),
    ("filter", "The grid's `WHERE`, as a fragment of SQL. With nothing said it is cleared."),
    ("sort", "The grid's `ORDER BY`, as a fragment of SQL. With nothing said it is cleared."),
    ("set", "Record a pending change: a row number from 1, a column name, and the value. `--null` sets NULL."),
    ("add-row", "Record a pending new row."),
    ("delete-row", "Record a pending deletion, by row number from 1."),
    ("pending", "The pending changes, and the statements they will become."),
    ("revert", "Throw the pending changes away."),
    ("submit", "Write them, as one transaction."),
    ("password", "Where a data source's password is: `<source> env <VARIABLE>`, `<source> keychain <entry>`, or `<source> none`. Never the password itself, which Quill does not store."),
    ("read-only", "`on` or `off` for a data source. The server enforces it; Quill also hides the controls."),
    ("confirm", "`on` or `off`: whether a console statement that changes rows is confirmed before it is sent. It asks a person; it never asks a command."),
    ("view", "Everything the pane and the tab are showing, as data."),
];

/// Run one.
pub fn run(explorer: &mut DatabaseExplorer, command: &str, arguments: &[String]) -> Result<Answer, String> {
    let rest = arguments.join(" ");
    let rest = rest.trim().to_owned();
    match command {
        "open-pane" | "open-tab" => Ok(Answer::said("the database plugin")),
        "sources" => sources(explorer),
        "add-source" => add_source(explorer, &rest),
        "remove-source" => {
            explorer.remove_source(rest.trim())?;
            Ok(Answer::said(format!("`{}` is gone", rest.trim())))
        }
        "connect" => {
            let name = a_source(explorer, &rest)?;
            explorer.connect(&name)?;
            Ok(Answer::said(format!(
                "connected to {}",
                explorer.version_of(&name).unwrap_or("the server")
            ))
            .with(serde_json::json!({ "source": name, "version": explorer.version_of(&name) })))
        }
        "disconnect" => {
            let name = a_source(explorer, &rest)?;
            explorer.disconnect(&name);
            Ok(Answer::said(format!("`{name}` is closed")))
        }
        "use" => {
            let name = a_source(explorer, &rest)?;
            explorer.configuration.chosen = name.clone();
            explorer.write_the_configuration()?;
            Ok(Answer::said(format!("pointed at `{name}`")))
        }
        "schemas" => schemas(explorer, &rest),
        "tables" => tables(explorer, &rest),
        "columns" => columns(explorer, &rest),
        "ddl" => ddl(explorer, &rest),
        "open" => open(explorer, &rest),
        "console" => {
            let name = a_source(explorer, &rest)?;
            let id = explorer.open_console(&name)?;
            Ok(Answer::said(format!("a console on `{name}`")).with(serde_json::json!({ "page": id })))
        }
        "query" => query(explorer, &rest),
        "state" => state(explorer),
        "result" => result(explorer),
        "stop" => {
            let id = current(explorer)?;
            explorer.stop(id)?;
            Ok(Answer::said("asked the server to stop"))
        }
        "reload" => {
            let id = current(explorer)?;
            explorer.reload(id);
            Ok(Answer::said("reading it again"))
        }
        "page" => page(explorer, &rest),
        "filter" => set_fragment(explorer, &rest, true),
        "sort" => set_fragment(explorer, &rest, false),
        "set" => set(explorer, arguments),
        "add-row" => {
            let grid = grid_mut(explorer)?;
            grid.pending.add();
            Ok(Answer::said("a row added, pending").with(serde_json::json!({ "pending": grid.pending.len() })))
        }
        "delete-row" => delete_row(explorer, &rest),
        "pending" => pending(explorer),
        "revert" => {
            let grid = grid_mut(explorer)?;
            grid.pending.clear();
            Ok(Answer::said("the pending changes are gone"))
        }
        "submit" => {
            let id = current(explorer)?;
            let count = explorer.submit(id)?;
            Ok(Answer::said(format!("{count} statement(s) sent as one transaction"))
                .with(serde_json::json!({ "statements": count })))
        }
        "password" => password(explorer, &rest),
        "read-only" => read_only(explorer, &rest),
        "confirm" => {
            match rest.trim() {
                "on" | "true" => explorer.configuration.confirm_writes = true,
                "off" | "false" => explorer.configuration.confirm_writes = false,
                // With nothing said it toggles, because the menu entry that could run it is a toggle
                // and a menu row cannot carry an argument — the shape Agent-Chat's `tools` already has.
                "" => explorer.configuration.confirm_writes = !explorer.configuration.confirm_writes,
                other => return Err(format!("confirm takes `on` or `off`, not `{other}`.")),
            }
            explorer.write_the_configuration()?;
            Ok(Answer::said(match explorer.configuration.confirm_writes {
                true => "a console statement that changes rows is confirmed first",
                false => "nothing is confirmed",
            })
            .with(serde_json::json!({ "confirm_writes": explorer.configuration.confirm_writes })))
        }
        "view" => Ok(Answer::said("the database plugin").with(explorer.view())),
        other => Err(format!(
            "`{other}` is not one of the Database plugin's commands. It has {}.",
            LIST.iter().map(|(name, _)| *name).collect::<Vec<&str>>().join(", ")
        )),
    }
}

fn sources(explorer: &DatabaseExplorer) -> Result<Answer, String> {
    let names: Vec<&str> = explorer.sources().iter().map(|source| source.name.as_str()).collect();
    let value = explorer.view();
    Ok(Answer::said(match names.is_empty() {
        true => "no data sources yet".to_owned(),
        false => names.join(", "),
    })
    .with(value.get("sources").cloned().unwrap_or(serde_json::Value::Null)))
}

/// `add-source <name> <url or file>`, or the dialog when nothing is said.
///
/// **One path for the menu entry and the command**, which is the rule `run_action` keeps for the
/// window: `Database -> New Data Source` carries no arguments — a menu row cannot — so with nothing
/// said this opens the dialog a person fills in, and with a name and a URL it adds one outright.
fn add_source(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    if rest.trim().is_empty() {
        explorer.modal = Some(crate::services::database::Modal::Source(a_new_source(explorer)));
        return Ok(Answer::said("fill in the new data source"));
    }
    let (name, rest) = rest
        .split_once(char::is_whitespace)
        .ok_or_else(|| "add-source takes a name and a URL, or a name and the path of a SQLite file.".to_owned())?;
    // An optional third word: the name of an environment variable holding the password, so that
    // adding a source and saying where its password is are one command rather than two. Never the
    // password — see `password`.
    let (url, variable) = match rest.trim().rsplit_once(char::is_whitespace) {
        Some((url, variable)) if is_a_variable_name(variable) => (url, Some(variable.to_owned())),
        _ => (rest.trim(), None),
    };
    let source = Source::parse(name.trim(), url.trim()).map_err(|why| why.to_string())?;
    // **Read only until somebody says otherwise**, which is the same default the settings file and the
    // New Data Source dialog use. A source added from the command line by an agent is exactly the one
    // that should not be able to write on its first statement.
    let source = Source {
        name: explorer.configuration.a_free_name(name.trim()),
        read_only: crate::services::database::config::DEFAULT_READ_ONLY,
        secret: match variable {
            Some(variable) => Secret::Environment(variable),
            None => Secret::None,
        },
        ..source
    };
    let named = source.name.clone();
    let where_it_points = source.where_it_points();
    explorer.save_source("", source)?;
    Ok(Answer::said(format!("`{named}` — {where_it_points}"))
        .with(serde_json::json!({ "name": named, "where": where_it_points })))
}

fn schemas(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let name = a_source(explorer, rest)?;
    explorer.connect(&name)?;
    // **It waits here, and only here.** Every other command that reaches the database answers with a
    // ticket, but a tree that could not be read without three round trips through `state` would be a
    // tree no agent would use. Introspection is bounded and fast, and this is a read.
    let loaded = wait_for_the_schemas(explorer, &name)?;
    Ok(Answer::said(loaded.join(", ")).with(serde_json::json!({ "source": name, "schemas": loaded })))
}

/// Draw the replies until this source's schemas have arrived, or give up.
///
/// Bounded by a deadline rather than looping for ever, because a server that never answers must not
/// hold the window: the caller is inside a frame.
fn wait_for_the_schemas(explorer: &mut DatabaseExplorer, name: &str) -> Result<Vec<String>, String> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        explorer.take_the_replies();
        if let Some(loaded) = explorer.loaded.get(name) {
            if let Some(why) = &loaded.problem {
                return Err(why.clone());
            }
            if !loaded.schemas.is_empty() {
                return Ok(loaded.schemas.clone());
            }
        }
        if std::time::Instant::now() > until {
            return Err(format!("`{name}` has not answered in ten seconds."));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn tables(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let source = explorer.configuration.chosen.clone();
    if source.is_empty() {
        return Err("there is no data source to look in. `use <name>` first.".to_owned());
    }
    explorer.connect(&source)?;
    let schemas = wait_for_the_schemas(explorer, &source)?;
    let schema = match rest.trim().is_empty() {
        true => schemas.first().cloned().unwrap_or_default(),
        false => rest.trim().to_owned(),
    };
    explorer.toggle_schema(&source, &schema);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        explorer.take_the_replies();
        if let Some(items) = explorer.loaded.get(&source).and_then(|loaded| loaded.items.get(&schema)) {
            let listed: Vec<serde_json::Value> = items
                .iter()
                .map(|item| serde_json::json!({ "name": item.name, "kind": item.kind.name() }))
                .collect();
            let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
            return Ok(Answer::said(names.join(", "))
                .with(serde_json::json!({ "source": source, "schema": schema, "items": listed })));
        }
        if let Some(why) = explorer.loaded.get(&source).and_then(|loaded| loaded.problem.clone()) {
            return Err(why);
        }
        if std::time::Instant::now() > until {
            return Err(format!("`{source}` has not answered in ten seconds."));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn columns(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let (schema, name) = split_a_name(explorer, rest)?;
    let source = explorer.configuration.chosen.clone();
    explorer.connect(&source)?;
    explorer.toggle_table(&source, &schema, &name);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        explorer.take_the_replies();
        if let Some(table) = explorer
            .loaded
            .get(&source)
            .and_then(|loaded| loaded.columns.get(&(schema.clone(), name.clone())))
        {
            return Ok(Answer::said(format!(
                "{} columns, key [{}]",
                table.columns.len(),
                table.key.join(", ")
            ))
            .with(serde_json::json!({
                "schema": table.schema,
                "table": table.name,
                "key": table.key,
                "editable": table.can_be_changed(),
                "columns": table.columns.iter().map(|column| serde_json::json!({
                    "name": column.name,
                    "type": column.type_name,
                    "not_null": column.not_null,
                    "key": column.in_key,
                })).collect::<Vec<serde_json::Value>>(),
            })));
        }
        if std::time::Instant::now() > until {
            return Err(format!("`{source}` has not answered in ten seconds."));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn ddl(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let (schema, name) = split_a_name(explorer, rest)?;
    let source = explorer.configuration.chosen.clone();
    explorer.connect(&source)?;
    let kind = explorer
        .loaded
        .get(&source)
        .and_then(|loaded| loaded.items.get(&schema))
        .and_then(|items| items.iter().find(|item| item.name == name))
        .map(|item| item.kind)
        .unwrap_or(quill_db::Kind::Table);
    // `show` is false: a command must never put a modal in front of somebody who asked for text.
    explorer.ask_for_ddl(&source, &schema, &name, kind, false)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        explorer.take_the_replies();
        if let Some((_, text)) = explorer.last_ddl.clone() {
            return Ok(Answer::said(text.clone()).with(serde_json::json!({ "table": name, "ddl": text })));
        }
        if std::time::Instant::now() > until {
            return Err(format!("`{source}` has not answered in ten seconds."));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn open(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let (schema, name) = split_a_name(explorer, rest)?;
    let source = explorer.configuration.chosen.clone();
    explorer.open_table(&source, &schema, &name)?;
    Ok(Answer::said(format!("opening {schema}.{name}"))
        .with(serde_json::json!({ "source": source, "schema": schema, "table": name })))
}

fn query(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    if rest.trim().is_empty() {
        return Err("query takes a statement.".to_owned());
    }
    // A console is opened if there is not one, so `query` works on a window nobody has touched.
    let id = match explorer.pages.get(explorer.current).map(|page| (page.id, matches!(page.sheet, Sheet::Console(_)))) {
        Some((id, true)) => id,
        _ => {
            let source = explorer.configuration.chosen.clone();
            explorer.open_console(&source)?
        }
    };
    if let Some(Page { sheet: Sheet::Console(console), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        console.text = rest.to_owned();
        console.caret = rest.len();
        console.result = None;
    }
    // Without the confirmation, for the reason `DatabaseExplorer::execute` records: an agent cannot
    // press a button in a modal, and the read-only switch is the guard that applies to both paths.
    let said = explorer.execute_now(id)?;
    Ok(Answer::said(format!("sent: {said}")).with(serde_json::json!({
        "page": id,
        "statement": said,
        // Said in the answer as well as in the summary, because this is the one thing an agent has to
        // know about this command and a summary is easy to skip.
        "waiting": true,
        "next": "ask `state`, then `result`",
    })))
}

fn state(explorer: &mut DatabaseExplorer) -> Result<Answer, String> {
    explorer.take_the_replies();
    let id = current(explorer)?;
    let Some(page) = explorer.page(id) else { return Err("no such page.".to_owned()) };
    let (running, said) = match &page.sheet {
        Sheet::Console(console) => (
            console.running.is_some(),
            console.output.last().cloned().unwrap_or_default(),
        ),
        Sheet::Grid(grid) => (grid.running.is_some(), grid.rows.summary()),
    };
    Ok(Answer::said(match running {
        true => "running".to_owned(),
        false => said.clone(),
    })
    .with(serde_json::json!({ "page": id, "running": running, "said": said })))
}

fn result(explorer: &mut DatabaseExplorer) -> Result<Answer, String> {
    explorer.take_the_replies();
    let id = current(explorer)?;
    let value = explorer
        .view()
        .get("pages")
        .and_then(|pages| pages.as_array().cloned())
        .and_then(|pages| pages.into_iter().find(|page| page.get("id") == Some(&serde_json::json!(id))))
        .unwrap_or(serde_json::Value::Null);
    let said = match &value {
        serde_json::Value::Object(page) => page
            .get("result")
            .or_else(|| page.get("rows"))
            .and_then(|rows| rows.get("count"))
            .map(|count| format!("{count} rows"))
            .unwrap_or_else(|| "nothing".to_owned()),
        _ => "nothing".to_owned(),
    };
    Ok(Answer::said(said).with(value))
}

fn page(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let id = current(explorer)?;
    let at = {
        let grid = grid_mut(explorer)?;
        match rest.trim() {
            "next" | "" => grid.at + 1,
            "previous" | "back" => grid.at.saturating_sub(1),
            "first" => 0,
            number => number
                .parse::<usize>()
                .map_err(|_| format!("`{number}` is not `next`, `previous`, `first` or a page number."))?
                .saturating_sub(1),
        }
    };
    grid_mut(explorer)?.at = at;
    explorer.reload(id);
    Ok(Answer::said(format!("page {}", at + 1)).with(serde_json::json!({ "page": at + 1 })))
}

fn set_fragment(explorer: &mut DatabaseExplorer, rest: &str, is_where: bool) -> Result<Answer, String> {
    let id = current(explorer)?;
    {
        let grid = grid_mut(explorer)?;
        match is_where {
            true => grid.where_clause = rest.to_owned(),
            false => grid.order_by = rest.to_owned(),
        }
        grid.at = 0;
    }
    explorer.reload(id);
    Ok(Answer::said(match rest.trim().is_empty() {
        true => "cleared".to_owned(),
        false => rest.to_owned(),
    }))
}

/// `set <row> <column> <value>`, with `--null` for NULL.
fn set(explorer: &mut DatabaseExplorer, arguments: &[String]) -> Result<Answer, String> {
    let null = arguments.iter().any(|argument| argument == "--null" || argument == "null");
    let words: Vec<&String> = arguments.iter().filter(|argument| *argument != "--null").collect();
    let (row, column) = match (words.first(), words.get(1)) {
        (Some(row), Some(column)) => (row.as_str(), column.as_str()),
        _ => return Err("set takes a row number, a column name and a value.".to_owned()),
    };
    let at: usize = row
        .parse::<usize>()
        .map_err(|_| format!("`{row}` is not a row number; they start at 1."))?
        .checked_sub(1)
        .ok_or_else(|| "row numbers start at 1.".to_owned())?;
    let value = match null {
        true => Value::Null,
        false => Value::typed(words.iter().skip(2).map(|word| word.as_str()).collect::<Vec<&str>>().join(" ")),
    };
    let grid = grid_mut(explorer)?;
    if let Some(why) = crate::services::database::why_not(grid) {
        return Err(why);
    }
    let key = grid
        .row_of(at)
        .ok_or_else(|| format!("there is no row {} on this page.", at + 1))?;
    if grid.rows.column(column).is_none() {
        return Err(format!("`{column}` is not a column of `{}`.", grid.table.name));
    }
    grid.pending.set(key, column, value);
    Ok(Answer::said("pending").with(serde_json::json!({ "pending": grid.pending.len() })))
}

fn delete_row(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let at: usize = rest
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("`{}` is not a row number; they start at 1.", rest.trim()))?
        .checked_sub(1)
        .ok_or_else(|| "row numbers start at 1.".to_owned())?;
    let grid = grid_mut(explorer)?;
    if let Some(why) = crate::services::database::why_not(grid) {
        return Err(why);
    }
    let key = grid
        .row_of(at)
        .ok_or_else(|| format!("there is no row {} on this page.", at + 1))?;
    grid.pending.delete(key);
    Ok(Answer::said("pending").with(serde_json::json!({ "pending": grid.pending.len() })))
}

fn pending(explorer: &mut DatabaseExplorer) -> Result<Answer, String> {
    let id = current(explorer)?;
    let statements = explorer.preview(id)?;
    Ok(Answer::said(format!("{} statement(s)", statements.len())).with(serde_json::json!(statements
        .iter()
        .map(|statement| serde_json::json!({
            "sql": statement.sql,
            "values": statement.values.iter().map(|value| match value {
                Value::Null => serde_json::Value::Null,
                other => serde_json::Value::String(other.display()),
            }).collect::<Vec<serde_json::Value>>(),
            "what": statement.what,
        }))
        .collect::<Vec<serde_json::Value>>())))
}

/// `password <source> env <VARIABLE>`, `keychain <entry>`, or `none`.
///
/// **The place, never the value.** There is deliberately no way to give Quill a password on the
/// command line: it would be in a shell history, in a process list and in whatever log the caller
/// keeps, which is three copies of a secret Quill has gone to some trouble never to write down once.
/// A password typed into the New Data Source dialog lives in the process and nowhere else.
fn password(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let mut words = rest.split_whitespace();
    let name = words
        .next()
        .ok_or_else(|| "password takes a data source and `env <VARIABLE>`, `keychain <entry>` or `none`.".to_owned())?
        .to_owned();
    let secret = match words.next() {
        Some("env") | Some("environment") => {
            let variable = words.next().ok_or_else(|| "`env` takes the name of an environment variable.".to_owned())?;
            Secret::Environment(variable.to_owned())
        }
        Some("keychain") => {
            let entry = words.next().ok_or_else(|| "`keychain` takes the name of an entry.".to_owned())?;
            Secret::Keychain(entry.to_owned())
        }
        Some("none") => Secret::None,
        Some(other) => {
            return Err(format!(
                "`{other}` is not somewhere a password lives. Quill knows `env <VARIABLE>`, \
                 `keychain <entry>` and `none`, and it never stores the password itself."
            ))
        }
        None => return Err("password takes `env <VARIABLE>`, `keychain <entry>` or `none`.".to_owned()),
    };
    let described = secret.describe();
    let source = explorer
        .configuration
        .source_mut(&name)
        .ok_or_else(|| format!("there is no data source called `{name}`."))?;
    source.secret = secret;
    // The connection is closed, because a password is read at the moment one is opened: a source
    // whose password has moved has to be opened again for the change to mean anything.
    explorer.disconnect(&name);
    explorer.write_the_configuration()?;
    Ok(Answer::said(format!("`{name}` reads its password from {described}"))
        .with(serde_json::json!({ "source": name, "password": described })))
}

fn read_only(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let mut words = rest.split_whitespace();
    let name = words.next().map(str::to_owned).unwrap_or_else(|| explorer.configuration.chosen.clone());
    let value = match words.next() {
        Some("on") | Some("true") => true,
        Some("off") | Some("false") => false,
        None => return Err("read-only takes a data source and `on` or `off`.".to_owned()),
        Some(other) => return Err(format!("read-only takes `on` or `off`, not `{other}`.")),
    };
    let source = explorer
        .configuration
        .source_mut(&name)
        .ok_or_else(|| format!("there is no data source called `{name}`."))?;
    source.read_only = value;
    // The connection is closed, because the guarantee is the server's: a read-only session is chosen
    // when the connection opens, so a source whose switch has been changed has to be opened again for
    // the change to mean anything.
    explorer.disconnect(&name);
    explorer.write_the_configuration()?;
    Ok(Answer::said(format!(
        "`{name}` is {}",
        match value {
            true => "read only, enforced by the server",
            false => "writable",
        }
    )))
}

/// The page the commands act on, which is whichever one the workspace is showing.
fn current(explorer: &DatabaseExplorer) -> Result<u64, String> {
    explorer
        .pages
        .get(explorer.current)
        .map(|page| page.id)
        .ok_or_else(|| "there is no page open. `console <source>` or `open <table>` first.".to_owned())
}

fn grid_mut(explorer: &mut DatabaseExplorer) -> Result<&mut crate::services::database::Grid, String> {
    let id = current(explorer)?;
    match explorer.pages.iter_mut().find(|page| page.id == id) {
        Some(Page { sheet: Sheet::Grid(grid), .. }) => Ok(grid),
        _ => Err("the page showing is not a grid. `open <table>` first.".to_owned()),
    }
}

/// A data source by name, or the chosen one when nothing was said.
fn a_source(explorer: &DatabaseExplorer, rest: &str) -> Result<String, String> {
    let wanted = match rest.trim().is_empty() {
        true => explorer.configuration.chosen.clone(),
        false => rest.trim().to_owned(),
    };
    if wanted.is_empty() {
        return Err("there are no data sources yet. `add-source <name> <url>` first.".to_owned());
    }
    match explorer.configuration.source(&wanted) {
        Some(_) => Ok(wanted),
        None => Err(format!(
            "there is no data source called `{wanted}`. There is {}.",
            match explorer.sources().is_empty() {
                true => "none at all".to_owned(),
                false => explorer
                    .sources()
                    .iter()
                    .map(|source| source.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "),
            }
        )),
    }
}

/// `schema.table`, or `table` in the first schema that has been read.
fn split_a_name(explorer: &DatabaseExplorer, rest: &str) -> Result<(String, String), String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("that takes a table, as `schema.table` or just `table`.".to_owned());
    }
    if let Some((schema, name)) = rest.split_once('.') {
        return Ok((schema.to_owned(), name.to_owned()));
    }
    let source = &explorer.configuration.chosen;
    let schema = explorer
        .loaded
        .get(source)
        .and_then(|loaded| loaded.schemas.first().cloned())
        .unwrap_or_else(|| "public".to_owned());
    Ok((schema, rest.to_owned()))
}

/// Whether a word looks like the name of an environment variable rather than part of a path.
///
/// Upper case, digits and underscores, which is the shape every environment variable on every
/// platform has — and which no Windows path and no URL ends in, so `add-source tasks C:\\db\\x.db`
/// is not read as naming a variable called `x.db`.
fn is_a_variable_name(word: &str) -> bool {
    !word.is_empty()
        && word.chars().all(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_')
        && word.chars().any(|character| character.is_ascii_uppercase())
}

/// Build the form the New Data Source modal opens with.
pub fn a_new_source(explorer: &DatabaseExplorer) -> SourceForm {
    SourceForm {
        was: String::new(),
        source: Source {
            name: explorer.configuration.a_free_name("data source"),
            read_only: crate::services::database::config::DEFAULT_READ_ONLY,
            ..Source::default()
        },
        typed: String::new(),
        variable: String::new(),
        tested: None,
    }
}

/// Build the form for editing one that exists.
pub fn a_form_for(source: &Source) -> SourceForm {
    SourceForm {
        was: source.name.clone(),
        source: source.clone(),
        typed: String::new(),
        variable: match &source.secret {
            Secret::Environment(name) => name.clone(),
            _ => String::new(),
        },
        tested: None,
    }
}
