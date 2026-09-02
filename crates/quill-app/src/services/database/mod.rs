//! The Database plugin's own state: the data sources, the connections, the tree and the pages.
//!
//! `crates/quill-db` is the half that talks to a database and has never heard of a window; this is the
//! half that has never heard of a wire protocol. Between them is `quill_db::Worker`, a connection on a
//! thread of its own, so nothing here is ever called from inside a frame while a query is running.
//!
//! `tasks/task-1777-database-plugin-tdd.md` is the design and
//! `_agent_output/task-1777-database-plugin/intellij-research.md` is what it was measured against.
//!
//! ## Three things worth knowing before changing anything here
//!
//! **A reply is routed by its ticket.** `Worker::take` drains the channel, so every answer has to be
//! looked up in [`Connection::waiting`] and given to the thing that asked for it. A caller that threw
//! away the answers it was not waiting for would lose the second of two outstanding queries, which is
//! a fault `quill-db`'s own tests found before this file existed.
//!
//! **Introspection is lazy and one level at a time.** Opening a data source asks for its schemas,
//! opening a schema asks for its items, opening a table asks for its columns. A database with four
//! thousand tables in it is the reason.
//!
//! **A row can only be changed if it can be addressed**, and this file never works around that. The
//! grid's Add, Delete and Submit are absent when `catalog::Table::can_be_changed` is false, and
//! `quill_db::Pending::statements` refuses to compose a statement for such a table even if something
//! here asked it to.

pub mod config;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use quill_db::catalog::{Item, Kind, Table};
use quill_db::edit::{Pending, Row};
use quill_db::rows::{Failure, Rows};
use quill_db::source::{Engine, Source};
use quill_db::value::Value;
use quill_db::worker::{Job, Reply, Worker};

use crate::services::plugin_ui::{Answer, Context, Look, Request, UiProvider};

pub use config::{password_for, Configuration};

/// One connected data source.
pub struct Connection {
    pub worker: Worker,
    /// What each outstanding ticket was asked for.
    waiting: BTreeMap<u64, Wanted>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Connection")
            .field("version", &self.worker.version)
            .field("waiting", &self.waiting.len())
            .finish()
    }
}

/// What an outstanding job was for, so its answer can be given to the right thing.
#[derive(Debug, Clone)]
enum Wanted {
    Schemas,
    Items { schema: String },
    /// A table's columns, and what to do once they have arrived.
    Describe { schema: String, name: String, then: Then },
    /// A `CREATE` statement. `show` is true when a person pressed the button, and false when an
    /// agent asked for the text — a command must not put a modal in front of somebody.
    Ddl { name: String, show: bool },
    Rows { page: u64 },
    Written { page: u64 },
    Nothing,
}

/// Why a table was described.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    /// Fill in the tree's column rows.
    ShowColumns,
    /// Open a grid on it, which needs the key before the first statement can be written.
    OpenGrid,
}

/// What has been introspected for one data source.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub schemas: Vec<String>,
    /// The items of each schema that has been opened.
    pub items: BTreeMap<String, Vec<Item>>,
    pub columns: BTreeMap<(String, String), Table>,
    pub open_schemas: BTreeSet<String>,
    /// `(schema, folder)` — `tables`, `views`, `routines` and the rest.
    pub open_folders: BTreeSet<(String, String)>,
    pub open_tables: BTreeSet<(String, String)>,
    /// What went wrong reading this source, if anything.
    pub problem: Option<String>,
}

/// A console: a SQL editor and whatever its last statement answered.
#[derive(Debug, Clone, Default)]
pub struct Console {
    pub source: String,
    pub schema: String,
    pub text: String,
    /// Where the caret is, in bytes, which is what decides the statement Execute runs.
    pub caret: usize,
    pub result: Option<Rows>,
    pub failure: Option<Failure>,
    /// What a statement that returned no rows said, newest last.
    pub output: Vec<String>,
    pub running: Option<u64>,
}

/// A grid on one table's rows.
#[derive(Debug, Clone)]
pub struct Grid {
    pub source: String,
    pub table: Table,
    pub kind: Kind,
    /// A fragment of SQL somebody typed, sent to the server rather than filtered here.
    pub where_clause: String,
    pub order_by: String,
    /// Which page, from zero.
    pub at: usize,
    pub rows: Rows,
    pub pending: Pending,
    pub failure: Option<Failure>,
    pub running: Option<u64>,
    /// The cell the keyboard is on, as `(row, column)`.
    pub chosen: Option<(usize, usize)>,
    /// What is being typed into that cell, while it is being typed.
    pub editing: Option<String>,
}

impl Grid {
    /// Which row of the database this row of the grid is, by the values of the table's key.
    pub fn row_of(&self, at: usize) -> Option<Row> {
        let row = self.rows.rows.get(at)?;
        let key = self
            .table
            .key
            .iter()
            .map(|name| {
                let column = self.rows.column(name)?;
                Some(row.get(column)?.display())
            })
            .collect::<Option<Vec<String>>>()?;
        Some(Row::Keyed(key))
    }

    /// What a cell should show: whatever is pending on it, or what was read.
    pub fn cell(&self, at: usize, column: usize) -> (Value, bool) {
        let read = self.rows.rows.get(at).and_then(|row| row.get(column)).cloned().unwrap_or_default();
        let Some(name) = self.rows.columns.get(column).map(|column| column.name.clone()) else {
            return (read, false);
        };
        match self.row_of(at).and_then(|row| self.pending.value_of(&row, &name).cloned()) {
            Some(pending) => (pending, true),
            None => (read, false),
        }
    }
}

/// One page in the workspace tab.
#[derive(Debug, Clone)]
pub enum Sheet {
    Console(Console),
    Grid(Box<Grid>),
}

/// A page and the number it is known by.
///
/// A number rather than an index, because a page is closed by pressing its own cross and the indexes
/// of every page after it then move — which is how an answer to a query gets applied to somebody
/// else's grid.
#[derive(Debug, Clone)]
pub struct Page {
    pub id: u64,
    pub sheet: Sheet,
}

impl Page {
    pub fn title(&self) -> String {
        match &self.sheet {
            Sheet::Console(console) => match console.source.is_empty() {
                true => "console".to_owned(),
                false => format!("console [{}]", console.source),
            },
            Sheet::Grid(grid) => format!("{} [{}]", grid.table.name, grid.source),
        }
    }

    pub fn source(&self) -> &str {
        match &self.sheet {
            Sheet::Console(console) => &console.source,
            Sheet::Grid(grid) => &grid.source,
        }
    }

    /// Which job this page is waiting for, if any. What decides whether an answer is still wanted.
    pub fn running(&self) -> Option<u64> {
        match &self.sheet {
            Sheet::Console(console) => console.running,
            Sheet::Grid(grid) => grid.running,
        }
    }
}

/// What is chosen in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chosen {
    pub source: String,
    pub schema: String,
    pub name: String,
}

/// A modal this plugin has open.
#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
    /// Adding or editing a data source.
    Source(SourceForm),
    /// The statements Submit is about to send.
    Preview { page: u64 },
    /// A `CREATE` statement.
    Ddl { title: String, text: String },
    /// A statement from a console that changes rows, waiting to be confirmed.
    Confirm { page: u64, statement: String },
}

/// The fields of the New Data Source dialog.
///
/// IntelliJ's own General tab, cut to what applies: name, engine, host, port, database, user, where
/// the password is, ssl mode and read-only. Where its dialog offers `Save: Forever` for a password,
/// this one offers `until this window closes` — see `plugin.limitations`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceForm {
    /// The name it had before it was edited, or empty when it is new.
    pub was: String,
    pub source: Source,
    /// A password typed here, held in this process and written nowhere.
    pub typed: String,
    /// The name of an environment variable holding the password.
    pub variable: String,
    /// What Test Connection said, if it has been pressed.
    pub tested: Option<Result<String, String>>,
}

/// The plugin.
pub struct DatabaseExplorer {
    open: bool,
    folder: Option<PathBuf>,
    project: Option<PathBuf>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    pub configuration: Configuration,
    connections: BTreeMap<String, Connection>,
    pub loaded: BTreeMap<String, Loaded>,
    pub open_sources: BTreeSet<String>,
    pub pages: Vec<Page>,
    pub current: usize,
    pub chosen: Option<Chosen>,
    pub filter: String,
    /// The last thing that went wrong, drawn under the tree.
    pub problem: Option<String>,
    pub modal: Option<Modal>,
    next_page: u64,
    /// How far down the tree is scrolled, which the provider keeps so a zoom can correct it.
    pub scrolled: f32,
    pub scroll_to: Option<f32>,
    /// The last `CREATE` statement that was read, as `(name, text)`.
    pub last_ddl: Option<(String, String)>,
    /// What this provider wants the window to do, decided outside a draw.
    ///
    /// Drained once a frame through [`UiProvider::asking`]. Opening a page has to put the workspace
    /// tab in front whoever asked for it — the tree's own double click, the menu, or an agent running
    /// `plugins run database open` — and only the window can show a tab.
    asking: Vec<Request>,
}

impl std::fmt::Debug for DatabaseExplorer {
    /// Written by hand because the waker is a closure and no closure has a `Debug`. What is printed is
    /// what a test wants to see when an assertion about the plugin fails.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("DatabaseExplorer")
            .field("sources", &self.configuration.sources.len())
            .field("connected", &self.connections.keys().collect::<Vec<&String>>())
            .field("pages", &self.pages.len())
            .field("problem", &self.problem)
            .finish()
    }
}

impl Default for DatabaseExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseExplorer {
    pub fn new() -> Self {
        Self {
            open: false,
            folder: None,
            project: None,
            wake: None,
            configuration: Configuration::default(),
            connections: BTreeMap::new(),
            loaded: BTreeMap::new(),
            open_sources: BTreeSet::new(),
            pages: Vec::new(),
            current: 0,
            chosen: None,
            filter: String::new(),
            problem: None,
            modal: None,
            next_page: 1,
            scrolled: 0.0,
            scroll_to: None,
            last_ddl: None,
            asking: Vec::new(),
        }
    }

    // ------------------------------------------------------------------ data sources

    pub fn sources(&self) -> &[Source] {
        &self.configuration.sources
    }

    pub fn is_connected(&self, name: &str) -> bool {
        self.connections.contains_key(name)
    }

    /// What the server called itself, for a connected source.
    pub fn version_of(&self, name: &str) -> Option<&str> {
        self.connections.get(name).map(|one| one.worker.version.as_str())
    }

    pub fn is_busy(&self, name: &str) -> bool {
        self.connections.get(name).is_some_and(|one| one.worker.is_busy())
    }

    /// True while anything at all is running, which is what keeps the window drawing.
    pub fn anything_running(&self) -> bool {
        self.connections.values().any(|one| one.worker.is_busy())
    }

    /// Open a connection, and ask it for its schemas.
    ///
    /// The password is fetched here and handed straight to the worker; nothing keeps it.
    pub fn connect(&mut self, name: &str) -> Result<(), String> {
        if self.connections.contains_key(name) {
            return Ok(());
        }
        let source = self
            .configuration
            .source(name)
            .cloned()
            .ok_or_else(|| format!("there is no data source called `{name}`."))?;
        let password = password_for(&source);
        let worker = Worker::open(&source, password.as_deref(), self.wake.clone())
            .map_err(|why| why.to_string())?;
        self.connections.insert(name.to_owned(), Connection { worker, waiting: BTreeMap::new() });
        self.loaded.entry(name.to_owned()).or_default().problem = None;
        self.ask(name, Job::Schemas, Wanted::Schemas);
        Ok(())
    }

    /// Close a connection and forget what was read through it.
    ///
    /// The tree's expansion is kept, because reopening a source somebody had open at `public.tables`
    /// and putting them back at the top would be a pane that forgets what they were doing.
    pub fn disconnect(&mut self, name: &str) {
        self.connections.remove(name);
        if let Some(loaded) = self.loaded.get_mut(name) {
            loaded.schemas.clear();
            loaded.items.clear();
            loaded.columns.clear();
        }
    }

    /// Add or replace a data source, and write the file.
    pub fn save_source(&mut self, was: &str, source: Source) -> Result<(), String> {
        if source.name.trim().is_empty() {
            return Err("a data source needs a name.".to_owned());
        }
        if self
            .configuration
            .sources
            .iter()
            .any(|other| other.name == source.name && other.name != was)
        {
            return Err(format!("there is already a data source called `{}`.", source.name));
        }
        // A source that changed is reconnected rather than left on a connection to the old address.
        self.disconnect(was);
        match self.configuration.sources.iter().position(|other| other.name == was) {
            Some(at) => self.configuration.sources[at] = source.clone(),
            None => self.configuration.sources.push(source.clone()),
        }
        // **A rename takes everything that names the source with it.** The name is the key in five
        // places, and leaving four of them behind gives pages that report a data source which is not
        // connected and can never be, and a tree that has lost its expansion — which is exactly the
        // lifecycle fault the conventions warn about.
        if !was.is_empty() && was != source.name {
            self.rename(was, &source.name);
        }
        if self.configuration.chosen.is_empty() {
            self.configuration.chosen = source.name.clone();
        }
        self.write_the_configuration()
    }

    /// Move everything that names `was` onto `now`.
    fn rename(&mut self, was: &str, now: &str) {
        if let Some(loaded) = self.loaded.remove(was) {
            self.loaded.insert(now.to_owned(), loaded);
        }
        if self.open_sources.remove(was) {
            self.open_sources.insert(now.to_owned());
        }
        if self.configuration.chosen == was {
            self.configuration.chosen = now.to_owned();
        }
        if let Some(chosen) = self.chosen.as_mut().filter(|chosen| chosen.source == was) {
            chosen.source = now.to_owned();
        }
        for page in &mut self.pages {
            match &mut page.sheet {
                Sheet::Console(console) if console.source == was => console.source = now.to_owned(),
                Sheet::Grid(grid) if grid.source == was => grid.source = now.to_owned(),
                _ => {}
            }
        }
    }

    pub fn remove_source(&mut self, name: &str) -> Result<(), String> {
        if self.configuration.source(name).is_none() {
            return Err(format!("there is no data source called `{name}`."));
        }
        self.disconnect(name);
        self.loaded.remove(name);
        self.open_sources.remove(name);
        self.pages.retain(|page| page.source() != name);
        self.current = self.current.min(self.pages.len().saturating_sub(1));
        self.configuration.sources.retain(|source| source.name != name);
        if self.configuration.chosen == name {
            self.configuration.chosen =
                self.configuration.sources.first().map(|source| source.name.clone()).unwrap_or_default();
        }
        self.write_the_configuration()
    }

    pub fn write_the_configuration(&self) -> Result<(), String> {
        match &self.folder {
            Some(folder) => self.configuration.write(folder),
            // A window with no store — which is every test — keeps its sources in memory, which is
            // the rule `QuillApp::load_settings` sets: a test must not read or write the settings of
            // the person running it.
            None => Ok(()),
        }
    }

    // ------------------------------------------------------------------ the tree

    /// Open or close a data source's row.
    pub fn toggle_source(&mut self, name: &str) {
        if self.open_sources.contains(name) {
            self.open_sources.remove(name);
            return;
        }
        self.open_sources.insert(name.to_owned());
        if !self.is_connected(name) {
            if let Err(why) = self.connect(name) {
                self.loaded.entry(name.to_owned()).or_default().problem = Some(why.clone());
                self.problem = Some(why);
            }
        }
    }

    /// Open or close a schema, asking for its items the first time.
    pub fn toggle_schema(&mut self, source: &str, schema: &str) {
        let loaded = self.loaded.entry(source.to_owned()).or_default();
        if loaded.open_schemas.contains(schema) {
            loaded.open_schemas.remove(schema);
            return;
        }
        loaded.open_schemas.insert(schema.to_owned());
        if !loaded.items.contains_key(schema) {
            self.ask(source, Job::Items { schema: schema.to_owned() }, Wanted::Items { schema: schema.to_owned() });
        }
    }

    pub fn toggle_folder(&mut self, source: &str, schema: &str, folder: &str) {
        let loaded = self.loaded.entry(source.to_owned()).or_default();
        let key = (schema.to_owned(), folder.to_owned());
        if !loaded.open_folders.insert(key.clone()) {
            loaded.open_folders.remove(&key);
        }
    }

    /// Open or close a table's columns, asking for them the first time.
    pub fn toggle_table(&mut self, source: &str, schema: &str, name: &str) {
        let loaded = self.loaded.entry(source.to_owned()).or_default();
        let key = (schema.to_owned(), name.to_owned());
        if !loaded.open_tables.insert(key.clone()) {
            loaded.open_tables.remove(&key);
            return;
        }
        if !loaded.columns.contains_key(&key) {
            self.describe(source, schema, name, Then::ShowColumns);
        }
    }

    fn describe(&mut self, source: &str, schema: &str, name: &str, then: Then) {
        self.ask(
            source,
            Job::Describe { schema: schema.to_owned(), table: name.to_owned() },
            Wanted::Describe { schema: schema.to_owned(), name: name.to_owned(), then },
        );
    }

    /// Ask for a table's `CREATE` statement. `show` puts it in a modal; a command asks with `false`.
    pub fn ask_for_ddl(&mut self, source: &str, schema: &str, name: &str, kind: Kind, show: bool) -> Result<(), String> {
        self.last_ddl = None;
        let job = Job::Ddl { schema: schema.to_owned(), table: name.to_owned(), kind };
        match self.ask(source, job, Wanted::Ddl { name: name.to_owned(), show }) {
            Some(_) => Ok(()),
            None => Err(format!("`{source}` is not connected.")),
        }
    }

    /// Ask a source's worker for something, remembering what it was for.
    fn ask(&mut self, source: &str, job: Job, wanted: Wanted) -> Option<u64> {
        let connection = self.connections.get_mut(source)?;
        match connection.worker.ask(job) {
            Ok(ticket) => {
                connection.waiting.insert(ticket, wanted);
                Some(ticket)
            }
            Err(why) => {
                self.problem = Some(why.to_string());
                None
            }
        }
    }

    // ------------------------------------------------------------------ pages

    fn add_page(&mut self, sheet: Sheet) -> u64 {
        let id = self.next_page;
        self.next_page += 1;
        self.pages.push(Page { id, sheet });
        self.current = self.pages.len() - 1;
        // A page nobody can see is a page nobody asked for. Only the window can show a tab, so this is
        // asked for rather than done — the rule every `Request` keeps.
        self.asking.push(Request::ShowTab);
        id
    }

    pub fn page(&self, id: u64) -> Option<&Page> {
        self.pages.iter().find(|page| page.id == id)
    }

    fn page_mut(&mut self, id: u64) -> Option<&mut Page> {
        self.pages.iter_mut().find(|page| page.id == id)
    }

    pub fn close_page(&mut self, id: u64) {
        self.pages.retain(|page| page.id != id);
        self.current = self.current.min(self.pages.len().saturating_sub(1));
    }

    /// Open a console on a data source, which is what `Jump to query console` does in IntelliJ.
    pub fn open_console(&mut self, source: &str) -> Result<u64, String> {
        if self.configuration.source(source).is_none() {
            return Err(format!("there is no data source called `{source}`."));
        }
        let _ = self.connect(source);
        let schema = self
            .loaded
            .get(source)
            .and_then(|loaded| loaded.schemas.first().cloned())
            .unwrap_or_default();
        Ok(self.add_page(Sheet::Console(Console {
            source: source.to_owned(),
            schema,
            ..Console::default()
        })))
    }

    /// Open a grid on a table. Its columns are asked for first, because the key decides everything.
    pub fn open_table(&mut self, source: &str, schema: &str, name: &str) -> Result<(), String> {
        if self.configuration.source(source).is_none() {
            return Err(format!("there is no data source called `{source}`."));
        }
        self.connect(source)?;
        match self.loaded.get(source).and_then(|loaded| loaded.columns.get(&(schema.to_owned(), name.to_owned()))) {
            Some(table) => {
                let table = table.clone();
                self.grid_for(source, schema, table);
            }
            None => self.describe(source, schema, name, Then::OpenGrid),
        }
        Ok(())
    }

    /// Open a grid on a table that has already been described.
    ///
    /// `schema` is the one the **tree** files this table under, which is not always `table.schema`:
    /// SQLite's own `Table` carries no schema at all, while the tree files its items under `main`.
    /// Looking the kind up by the wrong one made every SQLite view read as an editable table.
    fn grid_for(&mut self, source: &str, schema: &str, table: Table) {
        let kind = self
            .loaded
            .get(source)
            .and_then(|loaded| loaded.items.get(schema))
            .and_then(|items| items.iter().find(|item| item.name == table.name))
            .map(|item| item.kind)
            .unwrap_or(Kind::Table);
        let id = self.add_page(Sheet::Grid(Box::new(Grid {
            source: source.to_owned(),
            table,
            kind,
            where_clause: String::new(),
            order_by: String::new(),
            at: 0,
            rows: Rows::default(),
            pending: Pending::default(),
            failure: None,
            running: None,
            chosen: None,
            editing: None,
        })));
        self.reload(id);
    }

    /// Fill a grid, or run a console's statement again.
    pub fn reload(&mut self, id: u64) {
        let Some(page) = self.page(id) else { return };
        let Sheet::Grid(grid) = &page.sheet else { return };
        let source = grid.source.clone();
        let engine = self.configuration.source(&source).map(|source| source.engine).unwrap_or(Engine::Postgres);
        let limit = self.configuration.page_size;
        let statement = select_for(grid, engine, limit, grid.at);
        let ticket = self.ask(&source, Job::Query { sql: statement, limit }, Wanted::Rows { page: id });
        if let Some(Page { sheet: Sheet::Grid(grid), .. }) = self.page_mut(id) {
            grid.running = ticket;
            grid.failure = None;
        }
    }

    /// Run the statement under the caret of a console, asking first if it changes rows.
    ///
    /// This is the **button's** path. `ask` is false for the command line, and that is a decision
    /// rather than an oversight: the confirmation exists for a person who typed `delete from member`
    /// meaning to type a `where` clause after it, and an agent cannot press a button in a modal — so
    /// raising one there would leave a command that can never finish. The guard for both paths is the
    /// **read-only switch**, which is on by default and enforced by the server.
    pub fn execute(&mut self, id: u64) -> Result<String, String> {
        self.execute_with(id, true)
    }

    /// The same, without the confirmation. See [`Self::execute`].
    pub fn execute_now(&mut self, id: u64) -> Result<String, String> {
        self.execute_with(id, false)
    }

    fn execute_with(&mut self, id: u64, ask: bool) -> Result<String, String> {
        let Some(Page { sheet: Sheet::Console(console), .. }) = self.page(id) else {
            return Err("that page is not a console.".to_owned());
        };
        let statement = quill_db::sql::at(&console.text, console.caret)
            .ok_or_else(|| "there is no statement to run.".to_owned())?;
        let sql = statement.to_send().to_owned();
        if sql.is_empty() {
            return Err("there is no statement to run.".to_owned());
        }
        let source = console.source.clone();
        let read_only = self.configuration.source(&source).is_some_and(|source| source.read_only);
        if read_only && !quill_db::sql::only_reads(&statement) {
            return Err(format!(
                "`{}` is read only, so `{}` is not sent. Clear `Read only` on the data source to \
                 change anything through it.",
                source,
                statement.verb()
            ));
        }
        // A statement that changes rows is confirmed once, because a console is where somebody types
        // `delete from member` meaning to type a `where` clause after it.
        if ask && self.configuration.confirm_writes && !quill_db::sql::only_reads(&statement) {
            self.modal = Some(Modal::Confirm { page: id, statement: sql });
            return Ok("waiting for you to confirm it".to_owned());
        }
        self.send_from_console(id, &sql);
        Ok(sql)
    }

    /// Send a console's statement, having decided it should be sent.
    pub fn send_from_console(&mut self, id: u64, sql: &str) {
        let Some(Page { sheet: Sheet::Console(console), .. }) = self.page(id) else { return };
        let source = console.source.clone();
        let limit = self.configuration.page_size;
        let ticket = self.ask(&source, Job::Query { sql: sql.to_owned(), limit }, Wanted::Rows { page: id });
        if let Some(Page { sheet: Sheet::Console(console), .. }) = self.page_mut(id) {
            console.running = ticket;
            console.failure = None;
        }
    }

    /// Stop whatever the page's data source is doing.
    pub fn stop(&mut self, id: u64) -> Result<(), String> {
        let Some(page) = self.page(id) else { return Err("no such page.".to_owned()) };
        let source = page.source().to_owned();
        match self.connections.get(&source) {
            Some(connection) => connection.worker.stop().map_err(|why| why.to_string()),
            None => Err(format!("`{source}` is not connected.")),
        }
    }

    /// Write a grid's pending changes as one transaction.
    pub fn submit(&mut self, id: u64) -> Result<usize, String> {
        let Some(Page { sheet: Sheet::Grid(grid), .. }) = self.page(id) else {
            return Err("that page has nothing to submit.".to_owned());
        };
        if grid.pending.is_empty() {
            return Err("there is nothing to submit.".to_owned());
        }
        let source = grid.source.clone();
        let engine = self.configuration.source(&source).map(|source| source.engine).unwrap_or(Engine::Postgres);
        let statements = grid.pending.statements(&grid.table, engine).map_err(|why| why.to_string())?;
        let count = statements.len();
        let ticket = self.ask(&source, Job::Write { statements }, Wanted::Written { page: id });
        if let Some(Page { sheet: Sheet::Grid(grid), .. }) = self.page_mut(id) {
            grid.running = ticket;
            grid.failure = None;
        }
        Ok(count)
    }

    /// The statements a grid's pending changes would become, for the preview modal.
    pub fn preview(&self, id: u64) -> Result<Vec<quill_db::Statement>, String> {
        let Some(Page { sheet: Sheet::Grid(grid), .. }) = self.page(id) else {
            return Err("that page has nothing pending.".to_owned());
        };
        let engine = self
            .configuration
            .source(&grid.source)
            .map(|source| source.engine)
            .unwrap_or(Engine::Postgres);
        grid.pending.statements(&grid.table, engine).map_err(|why| why.to_string())
    }

    // ------------------------------------------------------------------ replies

    /// Take whatever the workers have answered, and give each answer to whatever asked for it.
    ///
    /// Called once a frame from `UiProvider::catch_up`. It must be cheap when nothing has arrived,
    /// which it is: `try_recv` on each open connection and nothing else.
    pub fn take_the_replies(&mut self) -> bool {
        let names: Vec<String> = self.connections.keys().cloned().collect();
        let mut anything = false;
        for name in names {
            let answered = match self.connections.get(&name) {
                Some(connection) => connection.worker.take(),
                None => continue,
            };
            for one in answered {
                anything = true;
                let wanted = self
                    .connections
                    .get_mut(&name)
                    .and_then(|connection| connection.waiting.remove(&one.ticket))
                    .unwrap_or(Wanted::Nothing);
                self.apply(&name, one.ticket, wanted, one.answer);
            }
        }
        anything || self.anything_running()
    }

    fn apply(
        &mut self,
        source: &str,
        ticket: u64,
        wanted: Wanted,
        answer: quill_db::Answer<Reply>,
    ) {
        let failure = match &answer {
            Ok(_) => None,
            Err(why) => Some(why.clone()),
        };
        match (wanted, answer) {
            (Wanted::Schemas, Ok(Reply::Names(names))) => {
                let loaded = self.loaded.entry(source.to_owned()).or_default();
                loaded.schemas = names;
                loaded.problem = None;
            }
            (Wanted::Items { schema }, Ok(Reply::Items(items))) => {
                self.loaded.entry(source.to_owned()).or_default().items.insert(schema, items);
            }
            (Wanted::Describe { schema, name, then }, Ok(Reply::Table(table))) => {
                let asked = schema.clone();
                self.loaded
                    .entry(source.to_owned())
                    .or_default()
                    .columns
                    .insert((schema, name), table.clone());
                if then == Then::OpenGrid {
                    self.grid_for(source, &asked, table);
                }
            }
            (Wanted::Ddl { name, show }, Ok(Reply::Text(text))) => {
                self.last_ddl = Some((name.clone(), text.clone()));
                if show {
                    self.modal = Some(Modal::Ddl { title: name, text });
                }
            }
            (Wanted::Rows { page }, Ok(Reply::Rows(rows))) => self.rows_arrived(page, ticket, rows),
            (Wanted::Written { page }, Ok(Reply::Written(affected))) => {
                if self.page(page).and_then(Page::running) != Some(ticket) {
                    return;
                }
                if let Some(Page { sheet: Sheet::Grid(grid), .. }) = self.page_mut(page) {
                    grid.pending.clear();
                    grid.running = None;
                }
                self.problem = Some(format!(
                    "{} statement(s) written, {} row(s) changed.",
                    affected.len(),
                    affected.iter().sum::<u64>()
                ));
                // Read the rows back, because what is on the screen after a write should be what is
                // in the database rather than what was typed.
                self.reload(page);
            }
            (wanted, _) => {
                let Some(why) = failure else { return };
                self.failed(source, ticket, wanted, why);
            }
        }
    }

    /// A result arriving for a console or a grid.
    ///
    /// **A reply that is not the one this page is waiting for is thrown away.** A second query or a
    /// reload can be asked for while the first is still outstanding — the command line makes that
    /// easy — and the older answer arriving afterwards would otherwise overwrite the newer one, so
    /// the grid would end up showing the result of a statement nobody is looking at. The ticket is
    /// what says which, and it is the same reason `quill_db::Worker` hands one out at all.
    fn rows_arrived(&mut self, page: u64, ticket: u64, rows: Rows) {
        let Some(page) = self.page_mut(page) else { return };
        if page.running() != Some(ticket) {
            return;
        }
        match &mut page.sheet {
            Sheet::Console(console) => {
                {
                    console.running = None;
                }
                for notice in &rows.notices {
                    console.output.push(notice.clone());
                }
                match rows.columns.is_empty() {
                    // A statement that returned no rows fills `Output`, which is IntelliJ's own
                    // arrangement and is what tells an `UPDATE` from a `SELECT` of nothing.
                    true => console.output.push(rows.summary()),
                    false => {
                        console.output.push(rows.summary());
                        console.result = Some(rows);
                    }
                }
            }
            Sheet::Grid(grid) => {
                grid.running = None;
                grid.rows = rows;
                grid.chosen = None;
                grid.editing = None;
            }
        }
    }

    /// Something went wrong, and it is shown where the thing that asked for it is.
    ///
    /// A failure is dropped for the same reason a result is when it belongs to a superseded ticket:
    /// an old refusal landing on a page that is running something else would clear the spinner and
    /// report a statement nobody is waiting for.
    fn failed(&mut self, source: &str, ticket: u64, wanted: Wanted, why: Failure) {
        let said = why.to_string();
        match wanted {
            Wanted::Rows { page } | Wanted::Written { page } => {
                if let Some(page) = self.page_mut(page).filter(|page| page.running() == Some(ticket)) {
                    match &mut page.sheet {
                        Sheet::Console(console) => {
                            console.running = None;
                            console.output.push(said.clone());
                            console.failure = Some(why);
                        }
                        Sheet::Grid(grid) => {
                            grid.running = None;
                            grid.failure = Some(why);
                        }
                    }
                }
            }
            _ => {
                self.loaded.entry(source.to_owned()).or_default().problem = Some(said.clone());
            }
        }
        self.problem = Some(said);
    }

    /// Hand a page a result as though a worker had answered it. Tests only.
    ///
    /// `UiProvider::as_any_mut` exists for exactly two callers and a test is one of them; these two
    /// are the same idea for a path that otherwise needs a real server to be slow at the right moment.
    #[cfg(test)]
    pub fn take_a_result_for_tests(&mut self, page: u64, ticket: u64, rows: Rows) {
        self.rows_arrived(page, ticket, rows);
    }

    /// Say which job a page is waiting for. Tests only.
    #[cfg(test)]
    pub fn mark_running_for_tests(&mut self, page: u64, ticket: u64) -> Option<u64> {
        match self.page_mut(page)? {
            Page { sheet: Sheet::Console(console), .. } => console.running = Some(ticket),
            Page { sheet: Sheet::Grid(grid), .. } => grid.running = Some(ticket),
        }
        Some(ticket)
    }

    // ------------------------------------------------------------------ what it is showing

    fn view_value(&self) -> serde_json::Value {
        serde_json::json!({
            "sources": self.configuration.sources.iter().map(|source| serde_json::json!({
                "name": source.name,
                "engine": source.engine.name(),
                "url": source.url(),
                "read_only": source.read_only,
                "password": source.secret.describe(),
                "connected": self.is_connected(&source.name),
                "version": self.version_of(&source.name),
                "busy": self.is_busy(&source.name),
                "problem": self.loaded.get(&source.name).and_then(|loaded| loaded.problem.clone()),
            })).collect::<Vec<serde_json::Value>>(),
            "chosen": self.configuration.chosen,
            "page_size": self.configuration.page_size,
            "confirm_writes": self.configuration.confirm_writes,
            "tree": self.loaded.keys().map(|name| serde_json::json!({
                "source": name,
                "open": self.open_sources.contains(name),
                "schemas": self.loaded.get(name).map(|loaded| loaded.schemas.clone()).unwrap_or_default(),
                "open": self.loaded.get(name).map(|loaded| loaded.open_schemas.iter().cloned().collect::<Vec<String>>()).unwrap_or_default(),
                "items": self.loaded.get(name).map(|loaded| loaded.items.iter().map(|(schema, items)| serde_json::json!({
                    "schema": schema,
                    "items": items.iter().map(|item| serde_json::json!({ "name": item.name, "kind": item.kind.name() })).collect::<Vec<serde_json::Value>>(),
                })).collect::<Vec<serde_json::Value>>()).unwrap_or_default(),
            })).collect::<Vec<serde_json::Value>>(),
            "chosen_row": self.chosen.as_ref().map(|chosen| serde_json::json!({
                "source": chosen.source, "schema": chosen.schema, "name": chosen.name,
            })),
            "pages": self.pages.iter().map(|page| self.page_value(page)).collect::<Vec<serde_json::Value>>(),
            "current": self.pages.get(self.current).map(|page| page.id),
            "problem": self.problem,
        })
    }

    fn page_value(&self, page: &Page) -> serde_json::Value {
        match &page.sheet {
            Sheet::Console(console) => serde_json::json!({
                "id": page.id,
                "kind": "console",
                "title": page.title(),
                "source": console.source,
                "schema": console.schema,
                "text": console.text,
                "running": console.running.is_some(),
                "output": console.output,
                "result": console.result.as_ref().map(rows_value),
                "failure": console.failure.as_ref().map(std::string::ToString::to_string),
            }),
            Sheet::Grid(grid) => serde_json::json!({
                "id": page.id,
                "kind": "grid",
                "title": page.title(),
                "source": grid.source,
                "schema": grid.table.schema,
                "table": grid.table.name,
                "key": grid.table.key,
                "editable": grid.table.can_be_changed() && grid.kind.can_be_changed(),
                "why_not_editable": why_not(grid),
                "where": grid.where_clause,
                "order_by": grid.order_by,
                "page": grid.at,
                "running": grid.running.is_some(),
                "pending": grid.pending.len(),
                "rows": rows_value(&grid.rows),
                "failure": grid.failure.as_ref().map(std::string::ToString::to_string),
            }),
        }
    }
}

/// Why a grid cannot be edited, in one sentence, or nothing when it can.
pub fn why_not(grid: &Grid) -> Option<String> {
    if !grid.kind.can_be_changed() {
        return Some(format!(
            "`{}` is a {}, and its rows belong to the tables underneath it.",
            grid.table.name,
            grid.kind.name()
        ));
    }
    grid.table.why_not_changeable()
}

/// A result as data, bounded, for `plugins view` and for a test.
fn rows_value(rows: &Rows) -> serde_json::Value {
    serde_json::json!({
        "columns": rows.columns.iter().map(|column| serde_json::json!({
            "name": column.name,
            "type": column.type_name,
            "key": column.in_key,
        })).collect::<Vec<serde_json::Value>>(),
        // Bounded, because an agent handed three thousand rows to learn how many there are stops
        // asking — `task-1704`'s rule about proportionate replies.
        "rows": rows.rows.iter().take(50).map(|row| row.iter().map(|value| match value {
            Value::Null => serde_json::Value::Null,
            other => serde_json::Value::String(other.display()),
        }).collect::<Vec<serde_json::Value>>()).collect::<Vec<Vec<serde_json::Value>>>(),
        "count": rows.rows.len(),
        "more": rows.more,
        "affected": rows.affected,
        "tag": rows.tag,
        "elapsed_ms": rows.elapsed.as_millis() as u64,
        "notices": rows.notices,
    })
}

/// The statement that fills a grid.
///
/// `WHERE` and `ORDER BY` are fragments somebody typed, pasted in as they are: they are part of the
/// statement's own grammar and cannot be parameters in any engine, and pretending otherwise would
/// mean building a filter language IntelliJ's own users go round anyway. Every **value** the grid
/// writes back is bound — see `quill_db::edit`.
///
/// ## With no `ORDER BY` typed, the key is the order, and that is a correctness fix rather than a
/// tidiness one
///
/// `LIMIT … OFFSET` over an unordered result is not a page sequence: no engine promises an order it
/// was not asked for, so page two can repeat a row from page one and skip another entirely. It shows
/// up sooner than that, though, and it showed up on the first real PostgreSQL run of this code: an
/// `UPDATE` moves a row to the end of the heap, so reloading after a submit put the row that had just
/// been edited at the bottom and the grid appeared to shuffle itself. A row number then meant a
/// different row than it had a moment earlier, which is what `set <n>` and `delete-row <n>` are
/// addressed by.
///
/// So a table with a key is read **in key order** unless somebody has typed an order of their own.
/// A table with no key has nothing to order by, and its rows are read-only anyway.
pub fn select_for(grid: &Grid, engine: Engine, limit: usize, at: usize) -> String {
    // SQLite's implicit key is not in `select *`, so a table addressed by it has to ask for it by
    // name — and by **whichever** of its three names this table has not shadowed, which is what
    // `quill_db::sqlite::ROWID_ALIASES` decides. Asking for the literal `rowid` on a table that
    // declares a column of that name would select the declared column instead.
    let by_rowid = engine == Engine::Sqlite
        && grid
            .table
            .key
            .first()
            .is_some_and(|name| quill_db::sqlite::ROWID_ALIASES.contains(&name.as_str()));
    let columns = match by_rowid {
        true => format!("{}, *", grid.table.key[0]),
        false => "*".to_owned(),
    };
    let mut statement = format!("select {columns} from {}", grid.table.qualified('"'));
    if !grid.where_clause.trim().is_empty() {
        statement.push_str(&format!(" where {}", grid.where_clause.trim()));
    }
    match grid.order_by.trim() {
        typed if !typed.is_empty() => statement.push_str(&format!(" order by {typed}")),
        _ if !grid.table.key.is_empty() => {
            let by = grid
                .table
                .key
                .iter()
                .map(|name| quill_db::catalog::quoted(name, '"'))
                .collect::<Vec<String>>()
                .join(", ");
            statement.push_str(&format!(" order by {by}"));
        }
        _ => {}
    }
    // One more than is kept, which is what makes `1-200 of 200+` honest.
    statement.push_str(&format!(" limit {} offset {}", limit + 1, at * limit));
    statement
}

impl UiProvider for DatabaseExplorer {
    fn id(&self) -> &'static str {
        "database"
    }

    fn open(&mut self, context: &Context) -> Result<(), String> {
        self.folder = context.folder.clone();
        self.project = context.project.clone();
        self.wake = context.wake.clone();
        if let Some(folder) = &self.folder {
            let (configuration, refused) = Configuration::read(folder);
            self.configuration = configuration;
            if !refused.is_empty() {
                self.problem = Some(refused.join(" "));
            }
        }
        // **Nothing is connected at startup**, which is the laziness `UiProvider::open`'s own comment
        // describes: opening a pane must not open a socket to somebody's server. A source is
        // connected when its row in the tree is opened.
        self.open = true;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open
    }

    /// The wells, the raised cards and the gradient on Execute — the decoration `egui` cannot draw.
    fn draws_chrome(&self) -> bool {
        true
    }

    fn pane(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::database::pane(self, ui, look)
    }

    fn tab(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::database::tab(self, ui, look)
    }

    fn settings(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::database::settings_page::show(self, ui, look)
    }

    fn modal(&mut self, ctx: &egui::Context, look: &Look<'_>) -> (Vec<Request>, bool) {
        crate::components::database::modal::show(self, ctx, look)
    }

    fn command(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String> {
        crate::services::database::commands::run(self, command, arguments)
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        commands::LIST.to_vec()
    }

    fn view(&self) -> serde_json::Value {
        self.view_value()
    }

    fn catch_up(&mut self) -> bool {
        self.take_the_replies()
    }

    fn asking(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.asking)
    }

    fn zoomed(&mut self, ratio: f32, above: f32) {
        if !ratio.is_finite() || ratio <= 0.0 {
            return;
        }
        self.scroll_to = Some(((self.scrolled + above) * ratio - above).max(0.0));
    }

    fn showing(&mut self, project: Option<&std::path::Path>, _file: Option<&std::path::Path>) {
        self.project = project.map(std::path::Path::to_path_buf);
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn close(&mut self) {
        // Every connection goes, which is what `Worker`'s own `Drop` is for: a socket left open on
        // somebody's server after the pane that opened it has gone is a connection nobody can see.
        self.connections.clear();
        self.open = false;
    }
}

pub mod commands;

#[cfg(test)]
mod tests;
