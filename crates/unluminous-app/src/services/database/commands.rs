//! What the menu entries, the buttons in the pane and `unluminous-cli plugins run database …` all call.
//!
//! **One path**, which is `UnluminousApp::run_action`'s rule for the menus and `run_cli`'s for the command
//! line, kept here for a plugin: a thing done by hand and the same thing done by an agent are the same
//! call rather than two that agree today.
//!
//! Two of these deliberately do not wait. `query` starts a statement and answers with the ticket it
//! will come back under, because `UiProvider::command` runs inside a frame and a command that blocked
//! would stop the window drawing for the length of a query — the sentence `unluminous_git::Worker` exists
//! for, and the shape `run start` and `run output` already have. `state` says when it has finished and
//! `result` is where the rows are. Each summary says so, because an agent that does not know will ask
//! for the result too early exactly once.

use unluminous_db::source::{Secret, Source};
use unluminous_db::value::Value;

use crate::services::database::{ColumnForm, DatabaseExplorer, Page, Sheet, SourceForm, TableForm};
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
    ("password", "`<source> set <secret>` writes one into this machine's own credential store; `<source> keychain <entry>` points at an entry already there; `<source> none` forgets it. Nothing is ever written to a file but the *name* of the entry."),
    ("read-only", "`on` or `off` for a data source. Off by default. It asks the *server* for a session that cannot write, and there is no control for it in the window."),
    ("new-table", "Make a table. `<schema.name> <column>:<type>[:pk][:notnull] …`, or just a name to open the dialog."),
    ("drop-table", "Drop a table, by `schema.name`."),
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
        "new-table" => new_table(explorer, arguments),
        "drop-table" => drop_table(explorer, &rest),
        "password" => password(explorer, &rest),
        "read-only" => read_only(explorer, &rest),
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
    // Writable, which is the same default the settings file and the dialog now use — `task-1795`
    // asks for full access and there is no tick box anywhere to clear.
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

/// How long an introspecting command will wait for its answer before saying to ask again.
///
/// **A quarter of a second, because the caller is inside a frame.** `UiProvider::command` runs at the
/// top of a frame, so every millisecond spent here is a millisecond the window is not drawing — the
/// sentence `unluminous_git::Worker` exists for, and the reason `query` answers with a ticket instead.
/// Introspection is different in degree rather than in kind: it is one bounded catalogue query, it is
/// answered in single-digit milliseconds by a local server, and a tree that needed three round trips
/// through `state` to read one level is a tree no agent would use. So it waits, but only for as long
/// as a person would not notice, and past that it says to ask again rather than holding the window.
/// An earlier version waited **ten seconds**, which would have frozen the window on a slow server.
pub const PATIENCE: std::time::Duration = std::time::Duration::from_millis(250);

/// What a command says when the answer has not arrived inside [`PATIENCE`].
///
/// A refusal rather than an empty answer, because an empty list and "not yet" are different things and
/// an agent that could not tell them apart would report a database with no tables in it.
fn ask_again(source: &str) -> String {
    format!(
        "`{source}` has not answered yet. It is still being read — ask again in a moment; the window          is not held while it does."
    )
}

/// Wait for `ready` to answer, taking replies as they arrive, for at most [`PATIENCE`].
fn briefly(
    explorer: &mut DatabaseExplorer,
    source: &str,
    ready: impl Fn(&DatabaseExplorer) -> Option<Result<Answer, String>>,
) -> Result<Answer, String> {
    let until = std::time::Instant::now() + PATIENCE;
    loop {
        explorer.take_the_replies();
        if let Some(problem) = explorer.loaded.get(source).and_then(|loaded| loaded.problem.clone()) {
            return Err(problem);
        }
        if let Some(answer) = ready(explorer) {
            return answer;
        }
        if std::time::Instant::now() > until {
            return Err(ask_again(source));
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn schemas(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let name = a_source(explorer, rest)?;
    explorer.connect(&name)?;
    let source = name.clone();
    briefly(explorer, &name, move |explorer| {
        let loaded = explorer.loaded.get(&source)?;
        if loaded.schemas.is_empty() {
            return None;
        }
        let schemas = loaded.schemas.clone();
        Some(Ok(Answer::said(schemas.join(", "))
            .with(serde_json::json!({ "source": source, "schemas": schemas }))))
    })
}

fn tables(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let source = explorer.configuration.chosen.clone();
    if source.is_empty() {
        return Err("there is no data source to look in. `use <name>` first.".to_owned());
    }
    explorer.connect(&source)?;
    // The schemas first, because naming no schema means the first one — and a source that has not
    // answered with its schemas yet cannot say which that is.
    let known = {
        let source = source.clone();
        briefly(explorer, &source.clone(), move |explorer| {
            let loaded = explorer.loaded.get(&source)?;
            match loaded.schemas.is_empty() {
                true => None,
                false => Some(Ok(Answer::nothing().with(serde_json::json!(loaded.schemas.clone())))),
            }
        })?
    };
    let schema = match rest.trim().is_empty() {
        true => known
            .value
            .as_array()
            .and_then(|schemas| schemas.first())
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        false => rest.trim().to_owned(),
    };
    if !explorer.loaded.get(&source).is_some_and(|loaded| loaded.items.contains_key(&schema)) {
        explorer.toggle_schema(&source, &schema);
    }
    let wanted = schema.clone();
    let named = source.clone();
    briefly(explorer, &source, move |explorer| {
        let items = explorer.loaded.get(&named)?.items.get(&wanted)?;
        let listed: Vec<serde_json::Value> = items
            .iter()
            .map(|item| serde_json::json!({ "name": item.name, "kind": item.kind.name() }))
            .collect();
        let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
        Some(Ok(Answer::said(names.join(", "))
            .with(serde_json::json!({ "source": named, "schema": wanted, "items": listed }))))
    })
}

fn columns(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let (schema, name) = split_a_name(explorer, rest)?;
    let source = explorer.configuration.chosen.clone();
    explorer.connect(&source)?;
    if !explorer
        .loaded
        .get(&source)
        .is_some_and(|loaded| loaded.columns.contains_key(&(schema.clone(), name.clone())))
    {
        explorer.toggle_table(&source, &schema, &name);
    }
    let named = source.clone();
    let at = (schema, name);
    briefly(explorer, &source, move |explorer| {
        let table = explorer.loaded.get(&named)?.columns.get(&at)?;
        Some(Ok(Answer::said(format!(
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
        }))))
    })
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
        .unwrap_or(unluminous_db::Kind::Table);
    // `show` is false: a command must never put a modal in front of somebody who asked for text.
    explorer.ask_for_ddl(&source, &schema, &name, kind, false)?;
    briefly(explorer, &source.clone(), move |explorer| {
        let (_, text) = explorer.last_ddl.clone()?;
        Some(Ok(Answer::said(text.clone()).with(serde_json::json!({ "table": name, "ddl": text }))))
    })
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
/// **The place, never the value.** There is deliberately no way to give Unluminous a password on the
/// command line: it would be in a shell history, in a process list and in whatever log the caller
/// keeps, which is three copies of a secret Unluminous has gone to some trouble never to write down once.
/// A password typed into the New Data Source dialog lives in the process and nowhere else.
fn password(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let mut words = rest.split_whitespace();
    let name = words
        .next()
        .ok_or_else(|| "password takes a data source and `set <secret>`, `keychain <entry>` or `none`.".to_owned())?
        .to_owned();
    let secret = match words.next() {
        // **The one form that carries the secret itself**, and it carries it into the machine's own
        // credential store rather than into anything Unluminous writes. The value is the rest of the line,
        // so a password with a space in it is one argument.
        Some("set") => {
            let value = words.collect::<Vec<&str>>().join(" ");
            if value.is_empty() {
                return Err("`set` takes the password.".to_owned());
            }
            let entry = crate::services::database::keychain_entry_for(&name);
            crate::services::agent_tasks::keychain::write(&entry, &value)?;
            Secret::Keychain(entry)
        }
        Some("keychain") => {
            let entry = words.next().ok_or_else(|| "`keychain` takes the name of an entry.".to_owned())?;
            Secret::Keychain(entry.to_owned())
        }
        // Still read, because a data source somebody set up this way goes on working; there is no
        // longer a way to make a new one, and the dialog does not offer it.
        Some("env") | Some("environment") => {
            let variable = words.next().ok_or_else(|| "`env` takes the name of an environment variable.".to_owned())?;
            Secret::Environment(variable.to_owned())
        }
        Some("none") => {
            let _ = crate::services::agent_tasks::keychain::remove(
                &crate::services::database::keychain_entry_for(&name),
            );
            Secret::None
        }
        Some(other) => {
            return Err(format!(
                "`{other}` is not somewhere a password lives. Unluminous knows `set <secret>`, \
                 `keychain <entry>` and `none`."
            ))
        }
        None => return Err("password takes `set <secret>`, `keychain <entry>` or `none`.".to_owned()),
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
        tested: None,
    }
}

/// Build the form the New Table dialog opens with.
///
/// One column already there, a key called `id` of whichever integer type the engine makes count
/// itself up, because that is the first thing anybody types and getting it wrong is the commonest
/// fault in a table made in a hurry — see `Engine::key_column`.
pub fn a_new_table(explorer: &DatabaseExplorer, source: &str, schema: &str) -> TableForm {
    let engine = explorer
        .configuration
        .source(source)
        .map(|source| source.engine)
        .unwrap_or(unluminous_db::source::Engine::Postgres);
    TableForm {
        source: source.to_owned(),
        schema: schema.to_owned(),
        name: String::new(),
        columns: vec![ColumnForm {
            name: "id".to_owned(),
            type_name: match engine {
                unluminous_db::source::Engine::Sqlite => "INTEGER".to_owned(),
                unluminous_db::source::Engine::Postgres => "integer".to_owned(),
            },
            in_key: true,
            not_null: true,
        }],
        problem: None,
    }
}

/// `new-table <schema.name> <column>:<type>[:pk][:notnull] …`, or the dialog when only a place is
/// said.
///
/// **One path for the menu entry and the command**, which is the shape `add-source` already has: a
/// menu row cannot carry an argument, so with no columns after the name this opens the dialog a
/// person fills in, and with columns it makes the table outright.
fn new_table(explorer: &mut DatabaseExplorer, arguments: &[String]) -> Result<Answer, String> {
    let source = explorer.configuration.chosen.clone();
    if source.is_empty() {
        return Err("there is no data source to make a table in. `use <name>` first.".to_owned());
    }
    explorer.connect(&source)?;
    let where_it_goes = arguments.first().map(String::as_str).unwrap_or_default();
    // **A bare word that names a schema is the schema, not the table.** `new-table main` is what the
    // tree's own menu means by a right click on a schema — open the dialog *there* — and reading it
    // as `<name>` instead put `main` in the Name field and composed `CREATE TABLE "main"."main"`.
    // A name with a dot in it is unambiguous and is still read as `schema.table`. `task-1795`.
    let names_a_schema = !where_it_goes.is_empty()
        && !where_it_goes.contains('.')
        && explorer
            .loaded
            .get(&source)
            .is_some_and(|loaded| loaded.schemas.iter().any(|schema| schema == where_it_goes));
    let (schema, name) = match names_a_schema {
        true => (where_it_goes.to_owned(), String::new()),
        false => split_qualified(explorer, &source, where_it_goes)?,
    };
    if arguments.len() < 2 {
        let mut form = a_new_table(explorer, &source, &schema);
        form.name = name;
        explorer.modal = Some(crate::services::database::Modal::NewTable(form));
        return Ok(Answer::said("fill in the new table"));
    }
    let mut columns = Vec::new();
    for said in &arguments[1..] {
        columns.push(a_column(said)?);
    }
    let engine = explorer
        .configuration
        .source(&source)
        .map(|source| source.engine)
        .unwrap_or(unluminous_db::source::Engine::Postgres);
    let sql = unluminous_db::sql::create_table(&schema, &name, &columns, engine)?;
    explorer.run_the_ddl(&source, &schema, &sql)?;
    Ok(Answer::said(format!("`{name}` made")).with(serde_json::json!({ "sql": sql, "schema": schema, "table": name })))
}

/// `<name>:<type>[:pk][:notnull]`, which is the shortest thing that says what a column is.
fn a_column(said: &str) -> Result<ColumnForm, String> {
    let mut parts = said.split(':');
    let name = parts.next().unwrap_or_default().trim().to_owned();
    if name.is_empty() {
        return Err(format!("`{said}` names no column. A column is `name:type`, and `:pk` and `:notnull` may follow it."));
    }
    let type_name = parts.next().unwrap_or_default().trim().to_owned();
    if type_name.is_empty() {
        return Err(format!("`{name}` has no type. A column is `name:type`."));
    }
    let mut column = ColumnForm { name, type_name, in_key: false, not_null: false };
    for flag in parts {
        match flag.trim() {
            "pk" | "key" | "primary" => column.in_key = true,
            "notnull" | "not-null" | "nn" => column.not_null = true,
            "" => {}
            other => {
                return Err(format!(
                    "`{other}` is not something a column can be. `pk` and `notnull` are what may follow the type."
                ))
            }
        }
    }
    Ok(column)
}

/// `drop-table <schema.name>`.
fn drop_table(explorer: &mut DatabaseExplorer, rest: &str) -> Result<Answer, String> {
    let source = explorer.configuration.chosen.clone();
    if source.is_empty() {
        return Err("there is no data source to drop a table from. `use <name>` first.".to_owned());
    }
    explorer.connect(&source)?;
    let (schema, name) = split_qualified(explorer, &source, rest.trim())?;
    if name.is_empty() {
        return Err("drop-table takes `schema.table`, or just a table.".to_owned());
    }
    let sql = unluminous_db::sql::drop_table(&schema, &name)?;
    explorer.run_the_ddl(&source, &schema, &sql)?;
    Ok(Answer::said(format!("`{name}` is gone")).with(serde_json::json!({ "sql": sql })))
}

/// `schema.table` or just `table`, where a bare name means the source’s first schema.
fn split_qualified(
    explorer: &DatabaseExplorer,
    source: &str,
    said: &str,
) -> Result<(String, String), String> {
    let first = explorer
        .loaded
        .get(source)
        .and_then(|loaded| loaded.schemas.first().cloned())
        .unwrap_or_default();
    match said.split_once('.') {
        Some((schema, name)) => Ok((schema.trim().to_owned(), name.trim().to_owned())),
        None => Ok((first, said.trim().to_owned())),
    }
}

/// Build the form for editing one that exists.
pub fn a_form_for(source: &Source) -> SourceForm {
    SourceForm {
        was: source.name.clone(),
        source: source.clone(),
        typed: String::new(),
        tested: None,
    }
}
