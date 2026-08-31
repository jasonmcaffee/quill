//! The Agent-Tasks plugin: the board, its tickets, its terminals and its agents.
//!
//! `tasks/agent-tasks-plugin-tdd.md` is the design and `tasks/ui-plugin-architecture.md` is the plugin
//! system it sits in. This file is the [`crate::services::plugin_ui::UiProvider`] the manifest names:
//! it owns the state, answers the commands, and hands the drawing to `components::agent_tasks`.
//!
//! ## What is in each file
//!
//! `store.rs` is the SQLite file and every query there is. `model.rs` is what a ticket is. `board.rs`
//! is the arithmetic a drag and a search need. `watchdog.rs` is the two ways an agent stops.
//! `agent.rs` builds the command line Claude or Codex is launched with. `clock.rs` is the instant as
//! text. **None of them draws and none of them reads the window**, so all of it is tested with no
//! window, no database and no clock.
//!
//! ## Nothing is read once a frame
//!
//! The board is read when it is opened, when a command changed something, and when the two minute tick
//! fires. A frame in which nothing changed costs no query at all, which is `task-1666`'s rule kept the
//! way `symbols::Hover` keeps it. [`AgentTasks::refresh`] is the one place a read happens, so there is
//! no second path that could start reading per frame by accident.

pub mod agent;
pub mod board;
pub mod clock;
pub mod keychain;
pub mod model;
pub mod store;
pub mod watchdog;

use std::path::PathBuf;

use serde_json::json;

use crate::services::plugin_ui::{Answer, Context, Look, Request, UiProvider};
use model::{Assignee, Author, Board, Priority, Status, Task};
use store::{NewTask, Store, TaskEdit};

/// What the board is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Board,
    Backlog,
    Completed,
    Epics,
}

impl View {
    /// **Four, not five.** `task-28`: there was a `Schedule` view listing the rows in the `task_schedule`
    /// table with when each one next runs, and nothing on this board ever writes such a row — the browser
    /// board's scheduler is a server that runs while nobody is looking, which
    /// `tasks/agent-tasks-plugin-tdd.md` lists as absent. So it was a view of a table that is always empty.
    ///
    /// The table itself is still there and `store::schedules` still reads it. Dropping a table is deleting
    /// data, and this schema has never dropped anything.
    pub const ALL: [View; 4] = [View::Board, View::Backlog, View::Completed, View::Epics];

    pub fn name(self) -> &'static str {
        match self {
            View::Board => "board",
            View::Backlog => "backlog",
            View::Completed => "completed",
            View::Epics => "epics",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Board => "Board",
            View::Backlog => "Backlog",
            View::Completed => "Completed",
            View::Epics => "Epics",
        }
    }

    pub fn parse(name: &str) -> Option<View> {
        View::ALL.into_iter().find(|view| view.name() == name)
    }
}

/// The plugin's own settings, in `plugins/agent-tasks/settings.conf` beside its manifest.
///
/// Read by the same `store::Values` the window's own settings are read by, in the same `name = value`
/// format, so a person can correct one in a text editor.
///
/// **No secret is in this file.** An agent's authentication key goes to the machine's keychain, which is
/// what the board being replaced does and for the same reason: a settings file is copied between machines,
/// read by anything that can read the folder, and pasted into a bug report. `keychain.rs` is the code half,
/// and what the file holds is the *name* of the keychain entry rather than what is in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    /// Where the board is. Empty means [`store::Store::default_path`].
    pub database: Option<PathBuf>,
    /// The project folder a ticket's agent is launched in when the ticket names none.
    pub project: Option<PathBuf>,
    /// The agent a new ticket is assigned to.
    pub agent: Assignee,
    /// How long a lease is before the watchdog calls it expired.
    pub lease_minutes: i64,
    /// The model a new ticket is given, when it should not be the agent's own default.
    pub model: Option<String>,
    /// The effort a new ticket is given: `low`, `medium`, `high`, `xhigh` or `max`.
    pub effort: Option<String>,
    /// Which gateway the agent talks to, when it is not the one the agent ships pointing at.
    ///
    /// Passed to the agent as an environment variable, which is how both of them take it. It is a URL rather
    /// than a secret, so it lives in the file: somebody reading a settings file should be able to see which
    /// gateway their agents are talking to.
    ///
    /// `task-28`: this is Iliad's URL on a configuration that has never been written, because that is the
    /// gateway this machine uses and a field somebody has to paste a URL into before anything works is a
    /// field that stops them. **Empty means the agent's own endpoint** — `api.anthropic.com` for Claude and
    /// OpenAI's for Codex — which is what leaving both environment variables unset already does, and which
    /// is the ticket's "default url for that model".
    pub base_url: Option<String>,
}

/// The gateway this machine's agents talk to, which a configuration that has never been written starts at.
///
/// The same URL `~/.zshrc` exports as `ANTHROPIC_BASE_URL`. It is a URL rather than a secret, so it is
/// written here in the open, which is the same reason it is written to the settings file.
pub const ILIAD_URL: &str = "https://iliad-emerging-api.abbvienet.com/api/llm";

/// What the key is called in this machine's keychain, under the service `quill-agent-tasks`.
///
/// **A constant rather than a setting.** `task-28`: the page used to ask for a `Key name`, a `Key variable`
/// and then the key, which is three values to describe one connection and two of them are Quill's own
/// plumbing described to the person using it. There is one key, it is called this, and the variables it is
/// handed to the agent in are [`KEY_VARIABLES`].
pub const KEY_NAME: &str = "iliad";

/// Which environment variables the key is handed to the agent in.
///
/// All of the names that matter, because a name nothing reads costs nothing and a name the agent needed and
/// did not get is a board that cannot talk to the gateway. `~/.zshrc` sets `ANTHROPIC_API_KEY` and points
/// `ILIAD_API_KEY` at the same value for the Codex command line, and `OPENAI_API_KEY` is what Codex reads
/// when it is talking to an OpenAI compatible endpoint.
pub const KEY_VARIABLES: &[&str] = &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "ILIAD_API_KEY"];

impl Default for Configuration {
    fn default() -> Self {
        Self {
            database: None,
            project: None,
            agent: Assignee::Claude,
            lease_minutes: watchdog::Thresholds::default().lease_minutes,
            model: None,
            effort: None,
            base_url: Some(ILIAD_URL.to_owned()),
        }
    }
}

impl Configuration {
    const FILE: &'static str = "settings.conf";

    /// Read the configuration out of the plugin's folder, or the defaults when there is no file.
    ///
    /// A file that cannot be read is treated as a file that is not there, which is the rule
    /// `services::store` already keeps: a board that opened with its defaults is better than a board
    /// that refused to open because a settings line had a stray character in it.
    pub fn read(folder: &std::path::Path) -> Self {
        let Ok(text) = std::fs::read_to_string(folder.join(Self::FILE)) else {
            return Self::default();
        };
        let values = crate::services::store::Values::parse(&text);
        let path = |name: &str| {
            values
                .text(name)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        // The same reading as `path`, as text. One closure rather than the same three lines six times: a value
        // that is present and empty is a value nobody chose, which is the rule `Settings::shell` keeps.
        let said = |name: &str| {
            values.text(name).map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
        };
        Self {
            database: path("database"),
            project: path("project"),
            agent: values
                .text("agent")
                .and_then(Assignee::parse)
                .filter(|agent| agent.is_an_agent())
                .unwrap_or(Assignee::Claude),
            lease_minutes: values
                .number("lease")
                .map(|minutes| minutes as i64)
                .filter(|minutes| *minutes > 0)
                .unwrap_or_else(|| watchdog::Thresholds::default().lease_minutes),
            model: said("model"),
            effort: said("effort").filter(|level| EFFORTS.contains(&level.as_str())),
            // **Present and empty is not the same as absent.** A file with no `base-url` line has never been
            // written by this version, so it gets Iliad's URL; a file whose line is empty is one somebody
            // cleared on purpose, and clearing it means the agent's own endpoint. `write` always writes the
            // line, so the difference is a real one rather than an accident of which keys happen to be there.
            base_url: match values.text("base-url") {
                Some(url) => Some(url.trim().to_owned()).filter(|url| !url.is_empty()),
                None => Some(ILIAD_URL.to_owned()),
            },
        }
    }

    pub fn write(&self, folder: &std::path::Path) -> Result<(), String> {
        let mut values = crate::services::store::Values::new();
        // Written only once it has been chosen, which is the rule `Settings::shell` and the debug adapter
        // paths already keep: a settings file copied to another machine should name nothing it does not have
        // to. An empty line reads as a value of nothing rather than as no value.
        let mut set = |name: &str, value: Option<String>| {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                values.set(name, value);
            }
        };
        set("database", self.database.clone().map(|path| path.display().to_string()));
        set("project", self.project.clone().map(|path| path.display().to_string()));
        set("model", self.model.clone());
        set("effort", self.effort.clone());
        // Always written, even when it is empty, so that a person who cleared it gets the agent's own
        // endpoint rather than Iliad's URL back again. See `read`.
        values.set("base-url", self.base_url.clone().unwrap_or_default());
        values.set("agent", self.agent.name());
        values.set("lease", self.lease_minutes.to_string());
        std::fs::create_dir_all(folder)
            .map_err(|problem| format!("{} could not be made: {problem}", folder.display()))?;
        std::fs::write(
            folder.join(Self::FILE),
            values.to_text_headed(
                "The Agent-Tasks board. No secret is in here: the agent's authentication key lives in this \
                 machine's keychain under `iliad`, and `base-url` is the gateway it is used against. An empty \
                 `base-url` means whichever endpoint the agent itself is configured for.",
            ),
        )
        .map_err(|problem| format!("the settings could not be written: {problem}"))
    }

    /// The environment an agent is launched with, which is the base URL and the key.
    ///
    /// The key is read from the keychain at the moment of launch rather than held anywhere, so it is in this
    /// process for as long as it takes to hand it to a child and never written down. A key that cannot be read
    /// is left out rather than refused: both agents already know how to log in, and a board that would not
    /// start anything because a keychain entry had been renamed would be worse than one that lets the agent
    /// use its own credentials.
    ///
    /// ## Where the key ends up, said plainly
    ///
    /// In the agent process's own environment, and in the environment of everything that process starts. On
    /// macOS and Linux that is readable by any program running as the same user, which is the same reach a
    /// program would have to run `security find-generic-password` itself, so the keychain is not being
    /// undermined here — it is protecting the key from a copied settings file and from other users, and it does
    /// both. What it does not protect against is a program already running as you, and putting the key in an
    /// environment variable does not change that.
    ///
    /// Both agents read their key from the environment and neither reads it from a file, so there is no
    /// alternative to pass instead. Nothing Quill writes ever carries the value: the settings file holds the
    /// **name** of the keychain entry, `SessionSettings` prints its variable names and not their values, and the
    /// terminal's saved scrollback is what the program printed rather than what it was started with.
    pub fn environment(&self) -> Vec<(String, String)> {
        self.environment_given(the_key().as_deref())
    }

    /// The same, told what the key is rather than finding out.
    ///
    /// Split out so that what is built can be tested without a keychain and without touching the environment
    /// of the process running the tests: reading either would make the answer depend on the machine, and
    /// `std::env::set_var` in a test is a race against every other test in the binary. This is the shape
    /// `agent::launch` already has, which is why every command line in that module is a test with no terminal.
    pub fn environment_given(&self, key: Option<&str>) -> Vec<(String, String)> {
        let mut environment = Vec::new();
        if let Some(secret) = key {
            for variable in KEY_VARIABLES {
                environment.push(((*variable).to_owned(), secret.to_owned()));
            }
            // What the Iliad gateway itself wants, which is what `~/.zshrc` sets alongside the key. It carries
            // the key, so it is built here at the moment of launch with the rest and is never written down.
            environment.push(("ANTHROPIC_CUSTOM_HEADERS".to_owned(), format!("x-api-key: {secret}")));
        }
        if let Some(url) = &self.base_url {
            // Both agents read a base URL from the environment, and they read different names, so both are
            // set: a value nothing reads costs nothing and a value the agent needed and did not get is a
            // board that talks to the wrong gateway.
            environment.push(("ANTHROPIC_BASE_URL".to_owned(), url.clone()));
            environment.push(("OPENAI_BASE_URL".to_owned(), url.clone()));
        }
        environment
    }

    /// Where the board file is, whether or not one was configured.
    pub fn database_path(&self) -> PathBuf {
        self.database.clone().unwrap_or_else(Store::default_path)
    }
}

impl AgentTasks {
    /// Read the open ticket's description as markdown, or as its source.
    ///
    /// One function so the two buttons and `plugins run agent-tasks show` reach the same change by the same path,
    /// which is the rule every control on this board keeps. The scroll goes back to the top when the view
    /// changes, because a scroll measured in one view means nothing in the other.
    pub fn show_the_description_rendered(&mut self, rendered: bool) {
        self.detail.description_rendered = rendered;
        self.description_scroll = 0.0;
    }

    /// Read one comment as its source, or as markdown.
    pub fn show_the_comment_raw(&mut self, id: i64, raw: bool) {
        match raw {
            true => self.detail.comments_raw.insert(id),
            false => self.detail.comments_raw.remove(&id),
        };
    }

    /// Whether one comment is being read as its source. Comments start rendered, so this is false by default.
    pub fn the_comment_is_raw(&self, id: i64) -> bool {
        self.detail.comments_raw.contains(&id)
    }

    /// The folders a ticket's `Project` dropdown offers: this window's own, then the recent ones.
    ///
    /// `holding` is what the ticket's row already says, and it is kept wherever it is, for the reason
    /// `agent::models_for` keeps a model it does not know: a ticket may name a folder this window has never
    /// opened, and a dropdown that dropped it would change what the ticket says just by being drawn.
    pub fn known_projects(&self, holding: Option<&str>) -> Vec<String> {
        let mut offered: Vec<String> = Vec::new();
        let mut add = |path: String| {
            if !path.trim().is_empty() && !offered.contains(&path) {
                offered.push(path);
            }
        };
        if let Some(holding) = holding {
            add(holding.trim().to_owned());
        }
        if let Some(project) = &self.project {
            add(project.display().to_string());
        }
        for recent in &self.recent_projects {
            add(recent.display().to_string());
        }
        offered
    }
}

/// The agent's authentication key: the keychain first, then this process's own environment.
///
/// `task-28` asked for "the iliad key from zshrc", and both halves of that are needed because it depends on
/// how Quill was started. Quill launched from a terminal inherits `ANTHROPIC_API_KEY` from `~/.zshrc` and
/// there is nothing for anybody to type; Quill launched from the Dock inherits nothing, and the keychain is
/// where the key it uses has to have been put.
///
/// The keychain wins when both have one, because it is the value somebody chose here.
pub fn the_key() -> Option<String> {
    keychain::read(KEY_NAME).or_else(|| {
        std::env::var("ANTHROPIC_API_KEY").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
    })
}

/// Which of the two the key came from, for the Settings page to say. `None` when there is no key at all.
///
/// A page that said `set` without saying where from would leave somebody wondering why clearing the keychain
/// changed nothing, which is exactly what happens when the environment is the one answering.
pub fn where_the_key_came_from() -> Option<&'static str> {
    if keychain::read(KEY_NAME).is_some() {
        return Some("this machine's keychain");
    }
    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(value) if !value.trim().is_empty() => Some("ANTHROPIC_API_KEY in Quill's own environment"),
        _ => None,
    }
}

/// One ticket, open in the detail.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub task: Option<Task>,
    pub todos: Vec<model::Todo>,
    pub comments: Vec<model::Comment>,
    /// What is being typed into the comment box.
    pub draft: String,
    /// What is being typed into the new todo box.
    pub todo_draft: String,
    /// Which comment is being edited, and what is being typed into it.
    ///
    /// Two fields rather than one, because cancelling has to be able to leave the comment as it was: the draft is
    /// separate from the comment, so the block on screen is the draft while this is set and the stored body when
    /// it is not.
    pub editing_comment: Option<i64>,
    pub comment_edit: String,
    /// True while this is a ticket that was created by `+ Add Task` and has not been named.
    ///
    /// What the modal reads to know it is an editor rather than a view: its footer says `Discard` and `Done`
    /// rather than `Close`, and `Discard` deletes the row, because the row was created before anybody typed.
    pub is_new: bool,
    /// What is in the title field, which saves as it is typed.
    ///
    /// Held beside the ticket rather than read from it, because the field is what somebody is typing and the
    /// row is what has been saved. Reading the row into the field every frame would fight the keyboard.
    pub title_draft: String,
    /// True while the description is being read as markdown rather than written as its source.
    ///
    /// `task-28`. **Raw is the default**, because the description is the field somebody writes in and a
    /// description that had to be switched into an editable state before it could be typed into would be worse
    /// than one that is simply open. Rendered is a read of it.
    ///
    /// Not written to the database: this is how somebody is looking at a ticket right now, not a property of the
    /// ticket.
    pub description_rendered: bool,
    /// The comments being read as their source rather than as markdown, by id.
    ///
    /// The other way round from the description, and for the matching reason: a comment is read far more often
    /// than it is written, and an agent's comments are markdown with headings, lists and code in them. So a
    /// comment starts **rendered** and this holds the ones somebody has asked to see the source of.
    pub comments_raw: std::collections::HashSet<i64>,
}

/// The Agent-Tasks provider.
#[derive(Default)]
pub struct AgentTasks {
    store: Option<Store>,
    configuration: Configuration,
    /// The plugin's own folder, once it has been opened.
    folder: Option<PathBuf>,
    /// The window's project, which is where an agent is launched when a ticket names none.
    project: Option<PathBuf>,
    /// The folders this machine has had open, from `plugin_ui::Context`, which are the choices a ticket's
    /// `Project` dropdown offers.
    recent_projects: Vec<PathBuf>,
    board: Board,
    view: View,
    /// The ticket showing in the detail, or nothing while the lanes are showing.
    detail: Detail,
    /// What is typed in the search box, and what it found.
    query: String,
    results: Vec<Task>,
    /// What the pane last said, drawn under the header until the next command.
    message: String,
    /// The card being dragged, and which lane the pointer is over.
    pub dragging: Option<i64>,
    /// The Backlog and Completed views, read when one of them is showing and a command changed something.
    backlog: Vec<Task>,
    completed: Vec<Task>,
    /// How far the lanes are scrolled sideways.
    lane_scroll: f32,
    /// How far each lane is scrolled down, by lane. Four at most, so a list rather than a map.
    lane_downs: Vec<(Status, f32)>,
    /// How far the Backlog or Completed listing is scrolled down. One of the two is showing at a time.
    listing_down: f32,
    /// How far the open ticket's todo list is scrolled down.
    pub todo_scroll: f32,
    /// How far the rendered description is scrolled down.
    ///
    /// Beside `todo_scroll`, which is the same kind of value for the same reason: the painting is at absolute
    /// positions, so a scroll is a number this holds rather than something an `egui::ScrollArea` remembers.
    pub description_scroll: f32,
    /// The rendered markdown the open ticket is showing, kept between frames.
    ///
    /// Here rather than on `Detail` because a laid out page is not `Clone` and `Detail` is. It is drawing state,
    /// which is what `todo_scroll` and `modal_open` beside it already are.
    pub markdown: crate::components::markdown_text::Cache,
    /// What has been typed into the authentication key field, before it is saved to the keychain.
    ///
    /// Never written to a file and cleared as soon as it is saved, so it is in this process for as long as it
    /// takes somebody to press the button beside it.
    pub key_draft: String,
    /// Whether the key has been looked for since anything changed, and where it was found.
    ///
    /// Asked once rather than once a frame. See [`AgentTasks::where_the_key_is`].
    key_checked: Option<String>,
    key_source: Option<&'static str>,
    /// True once `Delete` has been pressed and is waiting to be pressed again.
    ///
    /// A second press rather than a question in a dialog of its own: deleting a ticket takes its todos and its
    /// comments with it, so it is the one control on the board that asks, and a column 260 points wide has room
    /// for a changed label and a `Keep it` beside it rather than for a tenth modal.
    pub delete_asked: bool,
    /// Which card the keyboard is on: a lane and how far down it.
    ///
    /// A lane and a row rather than a ticket's id, because the board changes under it — a card moved, a ticket
    /// deleted, a search narrowing the lanes — and a row that no longer exists is clamped to the last one there
    /// is, where an id that no longer exists would leave the keyboard on nothing. `None` until an arrow key is
    /// pressed, so a board nobody has touched with the keyboard draws no ring.
    pub chosen: Option<(Status, usize)>,
    /// Whether the ticket's Todos and Terminal sections have been shut.
    ///
    /// **Both open until somebody shuts one**, which is what the browser board does. The specification calls both
    /// collapsible and neither was: a person who wanted more room for the description or the comments could not
    /// get it, and on a short window the fixed budgets for these two were what pushed the other sections past the
    /// bottom edge. Held on the provider rather than in egui's memory because the pane and the modal are two
    /// places showing one ticket and they have to agree about it.
    /// Named for what shut means rather than for what open means, so that the default — `false`, which
    /// `#[derive(Default)]` gives every field on this provider — is both of them open.
    pub todos_shut: bool,
    pub terminal_shut: bool,
    /// True while the ticket modal is open.
    ///
    /// Separate from whether a ticket is open in the detail, because the pane shows a ticket in place and the
    /// modal shows the same ticket over everything: one ticket, two ways of looking at it.
    pub modal_open: bool,
    /// True while a selection is being dragged out in a ticket's terminal.
    pub terminal_selecting: bool,
    /// True when the ticket's terminal has the keyboard, so typing goes to the agent rather than to the
    /// board's own fields.
    pub terminal_focused: bool,
    /// A terminal for each ticket that has one, keyed by ticket id.
    terminals: Vec<TicketTerminal>,
    /// How a terminal asks the window to draw again, handed over when the provider is opened.
    ///
    /// `None` in a test, where nothing is drawing and a waker that does nothing is the right waker.
    waker: Option<quill_terminal::Waker>,
}

/// A ticket's own terminal.
///
/// A `quill_terminal::Session`, which is the same session and the same emulator the terminal tile
/// draws, so there is no second terminal stack inside a program that already has one. The board being
/// replaced runs a daemon in another process for this; §2.3 of the design says what that bought and
/// what replaces it.
pub struct TicketTerminal {
    pub task_id: i64,
    pub session: quill_terminal::Session,
    /// The conversation the agent in it is having, which is what a later resume names.
    pub session_id: String,
    /// The instant it last printed anything, so the watchdog can tell a working agent from a stopped
    /// one without reading the screen.
    pub last_output_at: String,
    /// Set while it has been paused, because a frozen process cannot answer and its silence means
    /// nothing.
    pub paused: bool,
    /// The lines waiting for the agent's prompt, oldest first, and the instant they may be typed.
    ///
    /// A fresh agent has not drawn its prompt yet, and characters typed before it does go nowhere, so the
    /// handoff waits rather than being sent with the spawn — and anything asked for while it waits waits
    /// behind it, in order, which is what keeps a comment from arriving before the line that says which
    /// ticket it is about. `Session` has no timer of its own, so the deadline is held here and
    /// [`TicketTerminal::pump`] is what checks it: one place rather than a thread per ticket.
    pending: Vec<String>,
    ready_at: std::time::Instant,
    /// How much the session had written the last time it was looked at, so "it printed something" is a
    /// comparison rather than a reading of the screen.
    written: usize,
}

impl std::fmt::Debug for TicketTerminal {
    /// Written by hand because `quill_terminal::Session` holds a channel and a thread and has no
    /// `Debug`, and the provider needs one so that a test can print it.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("TicketTerminal")
            .field("task_id", &self.task_id)
            .field("session_id", &self.session_id)
            .field("alive", &self.session.is_running())
            .field("paused", &self.paused)
            .finish()
    }
}

impl TicketTerminal {
    /// Read whatever the agent has written, send the handoff line once its prompt is ready, and record
    /// whether anything was printed.
    ///
    /// Called once a frame while the board is showing and once per watchdog tick, which is what makes
    /// `last_output_at` mean what the watchdog reads it as.
    pub fn pump(&mut self, now: &str) -> bool {
        self.session.pump();
        // The **last screenful** rather than the whole scrollback. `written_text(None)` rebuilds every line
        // the session has ever printed, which on a busy agent is megabytes, and this runs once a frame per
        // ticket. What the question needs is whether anything moved, and the tail answers that: an agent
        // that printed changed its last screenful, and one that printed exactly the same screenful twice is
        // an agent that printed.
        let tail = self.session.written_text(Some(TAIL_LINES));
        let written = tail.len() ^ (tail.as_bytes().iter().map(|byte| *byte as usize).sum::<usize>() << 8);
        let printed = written != self.written;
        if printed {
            self.written = written;
            self.last_output_at = now.to_owned();
        }
        let waiting = !self.pending.is_empty();
        if waiting && std::time::Instant::now() >= self.ready_at {
            for line in std::mem::take(&mut self.pending) {
                self.type_line(&line);
            }
        }
        printed || waiting
    }

    /// Type a line and press return, now.
    pub fn type_line(&mut self, line: &str) {
        self.session.send(line.as_bytes().to_vec());
        self.session.send(b"\r".to_vec());
    }

    /// Send a line, or queue it behind the handoff when the prompt is not ready yet.
    ///
    /// Answers whether it was queued. An agent that has just been started has not drawn its prompt, and
    /// characters typed before it does go nowhere, so a line written straight in would look sent and would
    /// not be. Everything waits in one queue, in the order it was asked for, which is also what keeps a
    /// comment from arriving before the handoff that says which ticket it is about.
    pub fn queue(&mut self, line: &str) -> bool {
        match self.pending.is_empty() {
            true => {
                self.type_line(line);
                false
            }
            false => {
                self.pending.push(line.to_owned());
                true
            }
        }
    }
}

impl std::fmt::Debug for AgentTasks {
    /// Written by hand because a `Waker` is a function and a `Session` holds a thread, and neither has
    /// a `Debug`. What is printed is what a test wants to see when an assertion fails.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("AgentTasks")
            .field("open", &self.is_open())
            .field("database", &self.configuration.database_path())
            .field("view", &self.view)
            .field("cards", &self.board.total())
            .field("terminals", &self.terminals.len())
            .finish()
    }
}

impl AgentTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// The board, for the drawing and for the tests.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// What the board is showing. Named `current_view` because [`UiProvider::view`] is the answer to a
    /// different question — what the pane holds, as data — and two functions called `view` on one type
    /// would be two things a reader has to tell apart.
    pub fn current_view(&self) -> View {
        self.view
    }

    /// Show one of the five views, reading whatever that view needs.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
        // The listings are read here rather than while drawing, so choosing the view is what costs the query.
        if let Err(problem) = self.read_the_listings() {
            self.message = problem;
        }
    }

    /// Read the Backlog and the Completed views, which are two queries the lanes do not need.
    ///
    /// Only for the view that is showing: a board looking at its lanes has no reason to know what is in its
    /// backlog, and reading both on every refresh would be two queries nobody asked for.
    fn read_the_listings(&mut self) -> Result<(), String> {
        match self.view {
            View::Backlog => self.backlog = self.store()?.backlog()?,
            View::Completed => self.completed = self.store()?.completed()?,
            _ => {}
        }
        Ok(())
    }

    pub fn detail(&self) -> &Detail {
        &self.detail
    }

    pub fn detail_mut(&mut self) -> &mut Detail {
        &mut self.detail
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[Task] {
        &self.results
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    /// The board that is actually open, as a person reads it.
    ///
    /// The **open** board rather than the configured path, because those differ in the case that matters:
    /// a window with no settings folder — which is every window a test builds — opens its board in memory,
    /// and a page that named a file on disk there would be naming a file nothing had written. It is also
    /// what keeps this machine's own path out of an accepted screenshot.
    pub fn board_where(&self) -> String {
        match self.store.as_ref() {
            Some(store) if store.path() == std::path::Path::new(":memory:") => {
                "in memory \u{2014} this window has no settings folder, so nothing is written".to_owned()
            }
            Some(store) => store.path().display().to_string(),
            None => "not open".to_owned(),
        }
    }

    /// The store, or a sentence saying the board is not open.
    ///
    /// Every command goes through this rather than unwrapping, because a provider whose `open` failed is
    /// a provider whose pane draws the reason rather than one that panics on the first click.
    fn store(&self) -> Result<&Store, String> {
        self.store.as_ref().ok_or_else(|| "the board is not open".to_owned())
    }

    /// Read the board again. The one place a query happens outside a command.
    pub fn refresh(&mut self) -> Result<(), String> {
        let board = self.store()?.board()?;
        self.board = board;
        if let Some(open) = self.detail.task.as_ref().map(|task| task.id) {
            self.open_detail(open)?;
        }
        if !self.query.is_empty() {
            self.results = self.store()?.search(&self.query)?;
        }
        self.read_the_listings()
    }

    /// Show one ticket in the detail, reading its todos and comments.
    pub fn open_detail(&mut self, id: i64) -> Result<(), String> {
        let store = self.store()?;
        let task = store.task(id)?;
        let (todos, comments) = match task.as_ref() {
            Some(task) => (store.todos(task.id)?, store.comments(task.id)?),
            None => (Vec::new(), Vec::new()),
        };
        let draft = std::mem::take(&mut self.detail.draft);
        let todo_draft = std::mem::take(&mut self.detail.todo_draft);
        // The title field keeps what is in it while the same ticket is open, and takes the row's title when a
        // different one is opened. Otherwise a refresh would overwrite what somebody was typing.
        let same = self.detail.task.as_ref().map(|open| open.id) == task.as_ref().map(|read| read.id);
        let title_draft = match same {
            true => std::mem::take(&mut self.detail.title_draft),
            false => task.as_ref().map(|read| read.title.clone()).unwrap_or_default(),
        };
        let is_new = self.detail.is_new && same;
        // A comment being edited survives a refresh of the same ticket and is dropped when a different one is
        // opened, on the same terms as the title field: a refresh happens every time anything on the board
        // changes, and one of those must not throw away what somebody has typed.
        let (editing_comment, comment_edit) = match same {
            true => (self.detail.editing_comment, std::mem::take(&mut self.detail.comment_edit)),
            false => (None, String::new()),
        };
        // How somebody is looking at this ticket, carried across a refresh the way the drafts above are: a refresh
        // happens whenever anything on the board changes, and a description that flipped back to its source
        // every time a todo was ticked would be unusable. Cleared when the ticket itself changes.
        let same_ticket = self.detail.task.as_ref().map(|open| open.id) == task.as_ref().map(|next| next.id);
        let (description_rendered, comments_raw) = match same_ticket {
            true => (self.detail.description_rendered, std::mem::take(&mut self.detail.comments_raw)),
            false => {
                self.markdown.forget();
                (false, std::collections::HashSet::new())
            }
        };
        // **The ring is clamped here, where the board changes**, not only when an arrow key moves it. A person who
        // ringed the last card in a lane and then deleted it, or typed a search that hid it, was left with a ring
        // on nothing: it vanished from the screen and Enter did nothing until another arrow was pressed.
        self.keep_the_ring_on_a_card();
        self.detail = Detail {
            task,
            todos,
            comments,
            draft,
            todo_draft,
            title_draft,
            is_new,
            editing_comment,
            comment_edit,
            description_rendered,
            comments_raw,
        };
        Ok(())
    }

    pub fn close_detail(&mut self) {
        self.detail = Detail::default();
        // A question nobody answered is forgotten rather than waiting for the next ticket to be opened.
        self.delete_asked = false;
    }

    /// Search, or clear the results when the query is empty.
    pub fn search(&mut self, query: &str) -> Result<(), String> {
        self.query = query.to_owned();
        self.results = match query.trim().is_empty() {
            true => Vec::new(),
            false => self.store()?.search(query)?,
        };
        Ok(())
    }

    /// The ticket a key names, or a sentence saying it is not on the board.
    fn by_key(&self, key: &str) -> Result<Task, String> {
        self.store()?
            .task_by_key(key)?
            .ok_or_else(|| format!("{key} is not on this board"))
    }

    /// Where an agent for this ticket is launched.
    ///
    /// The ticket's own project, then the one the settings name, then the folder this window has open.
    /// A window with no project and a ticket with none is refused rather than launching in whatever
    /// folder Quill happens to have started in.
    pub fn working_folder(&self, task: &Task) -> Result<PathBuf, String> {
        let named = task
            .project
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.configuration.project.clone())
            .or_else(|| self.project.clone());
        let folder = named.ok_or_else(|| {
            format!(
                "{} names no project and this window has no folder open, so there is nowhere to launch an agent",
                task.key
            )
        })?;
        match folder.is_dir() {
            true => Ok(folder),
            false => Err(format!("{} is not a folder", folder.display())),
        }
    }

    /// Start an agent on a ticket and hand it over.
    ///
    /// The claim is a guarded update, so two callers pressing start on one card do not both get it. The
    /// session id is chosen here and written to the row, which is what makes the conversation resumable
    /// after this Quill has closed.
    pub fn start(&mut self, key: &str) -> Result<Answer, String> {
        let task = self.by_key(key)?;
        let agent = match task.assignee.is_an_agent() {
            true => task.assignee,
            // A ticket the JIRA sync wrote is assigned to a person. Handing it to the configured agent
            // is what the board being replaced does, and it is the only way such a ticket reaches an
            // agent without somebody opening the detail and changing the assignee by hand.
            false => self.configuration.agent,
        };
        let folder = self.working_folder(&task)?;
        // A ticket that already has a session is already being worked, and starting it again would be a
        // second agent on one ticket. Which of the two things a person meant is not a guess: `Resume
        // session` is the other button, and it says so.
        // A ticket whose agent is **running** cannot be started again: that would be two agents on one ticket.
        if self.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running()) {
            return Err(format!("{key} already has an agent running"));
        }
        // A ticket whose agent has exited can be started again, and for a Codex ticket that is the **only** way
        // on: Codex names its own sessions, so its recorded id is Quill's marker and Resume refuses it. Refusing
        // Start as well left such a ticket trapped between two refusals with nothing to press.
        //
        // For Claude, Start on a ticket that has a session hands the same conversation back rather than
        // beginning a second one, because the id is one Claude answers to and losing it would lose the work.
        let resuming = task.session_id.is_some() && agent::can_resume(agent);
        let session = task
            .session_id
            .clone()
            .filter(|_| resuming)
            .unwrap_or_else(new_session_id);
        self.terminals.retain(|terminal| terminal.task_id != task.id || terminal.session.is_running());
        let plan = agent::Plan {
            agent,
            session: session.clone(),
            model: task.model.clone(),
            effort: task.effort.clone(),
            resuming,
        };
        let launch = agent::launch(&plan)?;
        let now = clock::now();
        // A fresh ticket is claimed; one being started again is already claimed and is recorded instead, which is
        // what keeps `claim` strict — only an unclaimed ticket can be claimed, so two windows cannot both take
        // one — while still letting a ticket whose agent exited be started again.
        if task.session_id.is_none() {
            let claimed = self.store()?.claim(task.id, &session, agent, &owner(), &now)?;
            if !claimed {
                return Err(format!("{key} was claimed by another window while this one was starting it"));
            }
        } else {
            self.store()?.set_session(task.id, &session, &now)?;
            self.store()?.heartbeat(task.id, None, &now)?;
        }
        // The claim moved the ticket and the spawn can still fail, so a spawn that fails gives the claim
        // back. Without that the ticket would sit in In Progress naming a process that never existed, and
        // the watchdog would strike it three times before handing it on.
        let handoff = match resuming {
            true => agent::resumed(agent, key),
            false => agent::handoff(agent, key),
        };
        if let Err(problem) = self.spawn(task.id, &session, &launch, &folder, handoff) {
            // Only a claim this call took is given back. A ticket that was already claimed keeps its session:
            // the work behind it is real and the failure was the spawn.
            if task.session_id.is_none() {
                self.store()?.release(task.id, &now)?;
            }
            self.refresh()?;
            return Err(problem);
        }
        self.refresh()?;
        self.message = format!("{key} handed to {} in {}", agent.name(), folder.display());
        Ok(Answer::said(self.message.clone()).with(json!({
            "task": key,
            "agent": agent.name(),
            "session": session,
            "resuming": false,
            "folder": folder.display().to_string(),
            "command": launch.line(),
        })))
    }

    /// Bring back a session that has been retired, without changing which lane the ticket is in.
    ///
    /// This is what `Resume session` means: a ticket in Agent Done whose session is resumed stays in
    /// Agent Done, because resuming a conversation is not a claim on the work.
    pub fn resume(&mut self, key: &str) -> Result<Answer, String> {
        let task = self.by_key(key)?;
        let agent = match task.assignee.is_an_agent() {
            true => task.assignee,
            false => return Err(format!("{key} is assigned to a person, so there is no session")),
        };
        // Codex names its own sessions, so the id on a Codex ticket is Quill's marker that a worker was
        // here and means nothing to Codex. Starting a fresh agent and calling it a resumed one would be
        // the one outcome every check on this board exists to prevent.
        if !agent::can_resume(agent) {
            return Err(agent::why_it_cannot_resume(key));
        }
        let session = task
            .session_id
            .clone()
            .ok_or_else(|| format!("{key} has never had a session, so there is nothing to resume"))?;
        // A terminal object that is **running**, not merely one that is there. A retired session leaves its object
        // behind so its last screen can still be read, and refusing on that meant a Claude ticket whose agent had
        // exited could not be resumed at all — which is the one thing Resume session exists for.
        if self.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running()) {
            return Ok(Answer::said(format!("{key} already has an agent running")));
        }
        // The dead one is dropped first, or two objects would claim one ticket.
        self.terminals.retain(|terminal| terminal.task_id != task.id || terminal.session.is_running());
        let folder = self.working_folder(&task)?;
        let launch = agent::launch(&agent::Plan {
            agent,
            session: session.clone(),
            model: task.model.clone(),
            effort: task.effort.clone(),
            resuming: true,
        })?;
        // The task protocol requires a resumed run to re-read the ticket and take its newest human comments
        // as the specification, so that is what it is told. An empty handoff was a resumed agent that had
        // been told nothing and would carry on from whatever it last remembered.
        self.spawn(task.id, &session, &launch, &folder, agent::resumed(agent, key))?;
        self.store()?.set_session(task.id, &session, &clock::now())?;
        self.message = format!("{key} resumed on session {session}");
        Ok(Answer::said(self.message.clone()).with(json!({
            "task": key,
            "session": session,
            "command": launch.line(),
        })))
    }

    /// Type a line into a ticket's agent, resuming a retired session first.
    ///
    /// Resuming first is the rule the board being replaced keeps, and the reason is exact: typing at a
    /// process that has exited writes to nothing, and the comment would look sent.
    pub fn send(&mut self, key: &str, line: &str) -> Result<Answer, String> {
        let task = self.by_key(key)?;
        // **Running**, not merely present. Typing at a session that has exited wrote into a closed pipe and
        // reported `sent`, which is the one thing a send must never do.
        let alive = self.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running());
        if !alive {
            // Resuming first is the rule, and for a Codex ticket resuming is refused, so the refusal says
            // what to press instead rather than typing at a process that is not there.
            self.resume(key)?;
        }
        let terminal = self
            .terminals
            .iter_mut()
            .find(|terminal| terminal.task_id == task.id && terminal.session.is_running())
            .ok_or_else(|| format!("{key} has no agent running to type into"))?;
        // Queued rather than written, and the queue is what the handoff already waits in. An agent that has
        // just been started has not drawn its prompt, and characters typed before it does go nowhere: a
        // line written straight in would look sent and would not be.
        let queued = terminal.queue(line);
        self.message = match queued {
            true => format!("{key} is still starting, so this is queued behind its handoff"),
            false => format!("sent to {key}"),
        };
        Ok(Answer::said(self.message.clone()).with(json!({"task": key, "queued": queued})))
    }

    fn spawn(
        &mut self,
        task_id: i64,
        session_id: &str,
        launch: &agent::Launch,
        folder: &std::path::Path,
        handoff: String,
    ) -> Result<(), String> {
        // Which agent this is, so the queue waits as long as that agent takes to draw its prompt.
        let agent_kind = match launch.program.as_str() {
            "codex" => Assignee::Codex,
            _ => Assignee::Claude,
        };
        let settings = quill_terminal::SessionSettings {
            shell: Some(launch.program.clone()),
            args: launch.arguments.clone(),
            working_directory: Some(folder.to_path_buf()),
            // The board passes the two variables an agent needs to reach it. They are not secrets: the
            // board is a file this process already has open, and the agent reads it through the command
            // line rather than over a socket.
            // The board's own file, so an agent can find the board it is working on, plus whatever the
            // configuration says about where the agent connects and what key it uses. The key is read from the
            // keychain here and handed straight to the child: it is in this process for as long as that takes.
            env: {
                let mut environment = vec![(
                    "QUILL_AGENT_TASKS".to_owned(),
                    self.configuration.database_path().display().to_string(),
                )];
                environment.extend(self.configuration.environment());
                environment
            },
        };
        let waker = self.waker.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
        let session = quill_terminal::Session::spawn(
            &settings,
            quill_terminal::Size::new(40, 120),
            waker,
        )
        .map_err(|problem| format!("{} could not be started: {problem}", launch.program))?;
        let pending = match handoff.is_empty() {
            true => Vec::new(),
            false => vec![handoff],
        };
        self.terminals.retain(|terminal| terminal.task_id != task_id);
        self.terminals.push(TicketTerminal {
            task_id,
            session,
            session_id: session_id.to_owned(),
            last_output_at: clock::now(),
            paused: false,
            pending,
            ready_at: std::time::Instant::now() + agent::ready_after(agent_kind),
            written: 0,
        });
        Ok(())
    }

    /// Run one command and keep its message, which is what a button in the pane does.
    ///
    /// The same `command` the menu entry and `quill-cli plugin run` go through, so a button and an agent
    /// reach the same code rather than two paths that agree today. The arguments are owned because a
    /// button has just cloned a key out of the board it is drawing.
    pub fn command_now(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String> {
        let answer = UiProvider::command(self, command, arguments);
        match &answer {
            Ok(said) if !said.message.is_empty() => self.message = said.message.clone(),
            Ok(_) => {}
            Err(problem) => self.message = problem.clone(),
        }
        answer
    }

    /// Open one ticket as a new one, which is what `+ Add Task` does.
    ///
    /// In the modal, because a new ticket needs six fields and a description and none of those fits in a pane
    /// 420 points wide. That is also the browser board's own answer.
    pub fn open_a_new_ticket(&mut self, id: i64) -> Result<(), String> {
        self.open_detail(id)?;
        self.detail.is_new = true;
        self.modal_open = true;
        Ok(())
    }

    /// Open one ticket in the modal, which is what a click on a card does when there is room for one.
    pub fn open_the_modal(&mut self, id: i64) -> Result<(), String> {
        self.open_detail(id)?;
        // **Not** a new ticket, whatever the detail last held. Opening a ticket that exists is a different thing
        // from `+ Add Task`, and the difference is what the footer says: `Close` rather than `Discard` and
        // `Done`. Without this, a ticket opened after a new one had been closed still read as an editor.
        self.detail.is_new = false;
        self.modal_open = true;
        self.delete_asked = false;
        Ok(())
    }

    /// The description of the ticket that is open, as text.
    pub fn description_text(&self) -> String {
        self.detail.task.as_ref().map(|task| task.description.clone()).unwrap_or_default()
    }

    /// Write a changed description onto the ticket.
    ///
    /// Every keystroke, which is what "saves as you type" means. There is no debounce because there is no
    /// network: writing one column of a local row is a hundred microseconds.
    pub fn save_the_description(&mut self, text: &str) -> Result<(), String> {
        let Some(id) = self.detail.task.as_ref().map(|task| task.id) else {
            return Ok(());
        };
        let edit = TaskEdit { description: Some(text.to_owned()), ..TaskEdit::default() };
        self.store()?.edit_task(id, &edit, &clock::now())?;
        self.refresh()
    }

    /// Delete the ticket that is open, which is what `Discard` and `Delete` both do.
    pub fn discard_the_ticket(&mut self) -> Result<(), String> {
        let Some(id) = self.detail.task.as_ref().map(|task| task.id) else {
            return Ok(());
        };
        self.store()?.delete_task(id)?;
        // The modal goes with the ticket. Clearing the detail and leaving `modal_open` set drew a modal over a
        // ticket that was not there.
        self.modal_open = false;
        self.close_detail();
        self.refresh()
    }

    /// Write one field of a ticket.
    ///
    /// **One function for every control in the modal's right column**, so seven dropdowns are seven calls to one
    /// place rather than seven paths that agree today. An empty value clears the field where the field can be
    /// cleared, which is what pressing the chosen option in a row of choices means.
    pub fn edit_field(&mut self, id: i64, field: Field) -> Result<(), String> {
        let empty = |value: &str| value.trim().is_empty();
        let mut edit = TaskEdit::default();
        match &field {
            Field::Assignee(value) => {
                edit.assignee = Some(Assignee::parse(value).ok_or_else(|| {
                    format!(
                        "there is no `{value}` assignee: {}",
                        Assignee::ALL.iter().map(|one| one.name()).collect::<Vec<&str>>().join(", ")
                    )
                })?);
            }
            Field::Priority(value) => {
                edit.priority = Some(Priority::parse(value).ok_or_else(|| {
                    format!("there is no `{value}` priority: low, medium or high")
                })?);
            }
            Field::Model(value) => {
                edit.model = Some((!empty(value)).then(|| value.trim().to_owned()));
            }
            Field::Effort(value) => {
                if !empty(value) && !EFFORTS.contains(&value.as_str()) {
                    return Err(format!(
                        "there is no `{value}` effort: {}",
                        EFFORTS.join(", ")
                    ));
                }
                edit.effort = Some((!empty(value)).then(|| value.trim().to_owned()));
            }
            Field::Project(value) => {
                edit.project = Some((!empty(value)).then(|| value.trim().to_owned()));
            }
            Field::JiraKey(value) => {
                edit.jira_key = Some((!empty(value)).then(|| value.trim().to_owned()));
            }
            Field::Epic(value) => {
                let epic = match empty(value) {
                    true => None,
                    false => Some(value.trim().parse::<i64>().map_err(|_| {
                        format!("`{value}` is not an epic: an epic is named by its number")
                    })?),
                };
                edit.epic_id = Some(epic);
            }
        }
        self.store()?.edit_task(id, &edit, &clock::now())?;
        self.refresh()
    }

    /// Write what is in the title field onto the ticket.
    ///
    /// Called on every keystroke, which is one small update per character and is what "saves as you type"
    /// means. The board being replaced does the same, and for the same reason: `+ Add task` creates the row
    /// before anybody has named it, so there is no Create button to forget to press.
    pub fn save_the_title(&mut self) -> Result<(), String> {
        let Some(id) = self.detail.task.as_ref().map(|task| task.id) else {
            return Ok(());
        };
        let title = self.detail.title_draft.clone();
        let edit = TaskEdit { title: Some(title.clone()), ..TaskEdit::default() };
        self.store()?.edit_task(id, &edit, &clock::now())?;
        // The board is read again so the card's title follows the field, and the open ticket keeps the field.
        self.refresh()
    }

    /// Remove one todo.
    pub fn remove_the_todo(&mut self, id: i64) -> Result<(), String> {
        self.store()?.delete_todo(id)?;
        self.refresh()
    }

    /// Send a comment that is already on the board to the ticket's agent.
    ///
    /// What the browser's `Send to terminal` on each comment does. It writes nothing new: the comment is already
    /// there, so this is only the delivery, which is the difference from `comment-send`.
    pub fn send_a_comment(&mut self, body: &str) -> Result<String, String> {
        let Some(task) = self.detail.task.clone() else {
            return Ok(String::new());
        };
        let line = agent::comment_handoff(&task.key, body);
        let answer = self.send(&task.key, &line)?;
        Ok(answer.message)
    }

    /// How far the todo list is scrolled, taking this frame's wheel into account.
    pub fn scroll_the_todos(&mut self, ui: &egui::Ui, area: egui::Rect, most: f32) -> f32 {
        if most <= 0.0 {
            self.todo_scroll = 0.0;
            return 0.0;
        }
        if ui.ctx().pointer_interact_pos().is_some_and(|at| area.contains(at)) {
            self.todo_scroll -= ui.ctx().input(|input| input.smooth_scroll_delta.y);
        }
        self.todo_scroll = self.todo_scroll.clamp(0.0, most);
        self.todo_scroll
    }

    /// Add the todo that is in the box, and clear it.
    pub fn post_the_todo(&mut self) -> Result<(), String> {
        let Some(id) = self.detail.task.as_ref().map(|task| task.id) else {
            return Ok(());
        };
        let text = self.detail.todo_draft.trim().to_owned();
        if text.is_empty() {
            return Ok(());
        }
        self.store()?.add_todo(id, &text, &clock::now())?;
        self.detail.todo_draft.clear();
        self.refresh()
    }

    /// Start editing a comment, putting what it says into the draft.
    ///
    /// Only a person's own comment can be edited, and the store refuses the rest, so this does not check: a
    /// button that is only drawn on a human's comment and a store that refuses an agent's is one rule kept in
    /// the place that cannot be got round.
    pub fn edit_the_comment(&mut self, id: i64) {
        let said = self
            .detail
            .comments
            .iter()
            .find(|comment| comment.id == id)
            .map(|comment| comment.body.clone())
            .unwrap_or_default();
        self.detail.comment_edit = said;
        self.detail.editing_comment = Some(id);
    }

    /// Leave the comment as it was.
    pub fn stop_editing_the_comment(&mut self) {
        self.detail.editing_comment = None;
        self.detail.comment_edit.clear();
    }

    /// Write what was typed into the comment being edited.
    pub fn save_the_comment(&mut self) -> Result<String, String> {
        let Some(id) = self.detail.editing_comment else {
            return Ok(String::new());
        };
        let body = self.detail.comment_edit.trim().to_owned();
        if body.is_empty() {
            return Err("a comment cannot be emptied; press Cancel to leave it as it was".to_owned());
        }
        self.store()?.edit_comment(id, &body, &clock::now())?;
        self.stop_editing_the_comment();
        self.refresh()?;
        Ok("the comment was changed".to_owned())
    }

    /// Post the comment that is in the box, and optionally type it into the ticket's agent.
    ///
    /// `to_the_agent` is the difference between the two buttons: posting writes it on the board, and sending
    /// writes it on the board **and** types it into the terminal, resuming a retired session first. Sending
    /// goes through the same `comment-send` command an agent uses, so a failed delivery posts nothing and says
    /// so rather than leaving a comment that looks sent.
    pub fn post_the_comment(&mut self, to_the_agent: bool) -> Result<String, String> {
        let Some(task) = self.detail.task.clone() else {
            return Ok(String::new());
        };
        let body = self.detail.draft.trim().to_owned();
        if body.is_empty() {
            return Ok(String::new());
        }
        let command = match to_the_agent {
            true => "comment-send",
            false => "comment",
        };
        let arguments = vec![task.key.clone(), body];
        let answer = self.command_now(command, &arguments)?;
        self.detail.draft.clear();
        Ok(answer.message)
    }

    /// Write a changed configuration to the plugin's own settings file.
    ///
    /// One function, so seven controls on the Settings page are seven calls to one place. Nothing here is a
    /// secret: the key is in the keychain and this file names it.
    pub fn change_the_configuration(&mut self, changed: Configuration) -> Result<(), String> {
        self.configuration = changed;
        match self.folder.clone() {
            Some(folder) => self.configuration.write(&folder),
            // A window with no settings folder — every window a test builds — keeps the change in memory and
            // writes nothing, which is the rule that stops a test touching the settings of the person running
            // it.
            None => Ok(()),
        }
    }

    /// Where the agent's key is coming from, read once rather than once a frame.
    ///
    /// Asking the keychain runs the platform's own tool and copies the secret into this process to answer, so a
    /// Settings page that asked while it drew spawned a process every frame and could provoke a keychain prompt on
    /// every one of them. The answer is remembered and forgotten when a key is written or taken away.
    ///
    /// `None` means there is no key anywhere, which is not an error: both agents already know how to log in, and
    /// a board that would not start anything until it had been told a key would be worse than one that lets the
    /// agent use its own credentials.
    pub fn where_the_key_is(&mut self) -> Option<&'static str> {
        if self.key_checked.is_none() {
            self.key_checked = Some(KEY_NAME.to_owned());
            self.key_source = where_the_key_came_from();
        }
        self.key_source
    }

    /// Whether there is a key at all, which is what a caller that only wants a yes or a no asks.
    pub fn the_key_is_set(&mut self) -> bool {
        self.where_the_key_is().is_some()
    }

    /// Forget what the keychain last said, because something has just changed it.
    fn the_key_changed(&mut self) {
        self.key_checked = None;
    }

    /// Put what was typed into the key field into the machine's keychain.
    ///
    /// The value is taken out of the draft rather than copied, so it is gone from this process as soon as the
    /// keychain has it. There is no name to give it: it is [`KEY_NAME`], because `task-28` asked for the
    /// minimum needed to connect and a name somebody has to invent is not part of that minimum.
    pub fn save_the_key(&mut self) -> Result<String, String> {
        let secret = self.key_draft.clone();
        if secret.trim().is_empty() {
            return Err("nothing was typed, so nothing was saved".to_owned());
        }
        // Written first and **then** forgotten. Taking it out of the draft before the write meant a keychain that
        // refused it left somebody with nothing typed and nothing saved.
        keychain::write(KEY_NAME, &secret)?;
        self.key_draft.clear();
        self.the_key_changed();
        Ok(format!("the key is in this machine's keychain as `{KEY_NAME}`"))
    }

    /// Take the key out of the keychain.
    ///
    /// This does not, and cannot, take away a key Quill inherited in its own environment. The page says which of
    /// the two answered, so that clearing the keychain and seeing `set` still shown is explained rather than
    /// puzzling.
    pub fn clear_the_key(&mut self) -> Result<String, String> {
        keychain::remove(KEY_NAME)?;
        self.the_key_changed();
        match std::env::var("ANTHROPIC_API_KEY").ok().filter(|value| !value.trim().is_empty()) {
            Some(_) => Ok(format!(
                "`{KEY_NAME}` is out of the keychain. Quill's own environment still has ANTHROPIC_API_KEY, and \
                 the agent will use that."
            )),
            None => Ok(format!("`{KEY_NAME}` is out of the keychain")),
        }
    }

    /// Choose the next agent, which is what the chooser under the New lane's heading does.
    ///
    /// A cycle rather than a dropdown, because egui keeps one popup open at a time and there are two agents
    /// that can be launched. It writes the same `agent` the Settings page writes, so the chooser on the board
    /// and the setting in the window can never disagree.
    pub fn use_the_next_agent(&mut self) -> Result<(), String> {
        let agents: Vec<Assignee> = Assignee::ALL.into_iter().filter(|agent| agent.is_an_agent()).collect();
        let at = agents.iter().position(|agent| *agent == self.configuration.agent).unwrap_or(0);
        let next = agents[(at + 1) % agents.len()];
        let mut changed = self.configuration.clone();
        changed.agent = next;
        self.change_the_configuration(changed)
    }

    /// The link to a JIRA issue, for the `Copy issue link` button on the ticket.
    ///
    /// The row's own `jira_url` when it has one, because a ticket that came from a sync carries the URL that
    /// sync read it from and that is the address that certainly works. Otherwise the key on its own: there is no
    /// configured JIRA site here to build a `/browse/` URL against, and a guessed address that opens nothing
    /// would be worse than the key, which somebody can paste into their own JIRA search.
    pub fn jira_link(&self, key: &str) -> String {
        self.detail
            .task
            .as_ref()
            .and_then(|task| task.jira_url.clone())
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| key.to_owned())
    }

    /// Move the keyboard's ring to another card, and say which ticket it is on now.
    ///
    /// `across` is lanes and `down` is rows. The counts come from the drawing, because they are the counts
    /// **after** the search has narrowed each lane: a ring that could land on a card the search had hidden would
    /// be a ring nobody could see.
    pub fn move_the_choice(&mut self, counts: &[(Status, usize)], across: i64, down: i64) {
        // Somewhere to start. The first lane that holds anything, so the first arrow press lands on a card rather
        // than on an empty New lane while three tickets sit in Agent Done.
        let Some(first) = counts.iter().find(|(_, held)| *held > 0).map(|(lane, _)| *lane) else {
            self.chosen = None;
            return;
        };
        // **The first press lands on the first card rather than moving from it.** A board with no ring on it has
        // nowhere to move from, so the press that starts is the press that chooses.
        let Some((lane, row)) = self.chosen else {
            self.chosen = Some((first, 0));
            return;
        };
        let mut at = Status::ALL.iter().position(|one| *one == lane).unwrap_or(0) as i64;
        let mut row = row as i64;
        if across != 0 {
            // Past the last lane stops at the last lane rather than wrapping. Wrapping in a row of four lanes
            // means a press that looks like it goes right sends the ring back to the left, which nobody expects.
            let held = |at: i64| {
                Status::ALL
                    .get(at.clamp(0, Status::ALL.len() as i64 - 1) as usize)
                    .and_then(|lane| counts.iter().find(|(one, _)| one == lane))
                    .map(|(_, held)| *held)
                    .unwrap_or(0)
            };
            let mut moved = at;
            // Empty lanes are stepped over, because stopping the ring on a lane with no cards in it would mean
            // pressing right twice to get past `QA FAILED 0`.
            loop {
                let next = moved + across;
                if next < 0 || next >= Status::ALL.len() as i64 {
                    break;
                }
                moved = next;
                if held(moved) > 0 {
                    at = moved;
                    break;
                }
            }
            row = row.min(held(at) as i64 - 1).max(0);
        }
        let lane = Status::ALL[at.clamp(0, Status::ALL.len() as i64 - 1) as usize];
        let held = counts.iter().find(|(one, _)| *one == lane).map(|(_, held)| *held).unwrap_or(0) as i64;
        if held == 0 {
            self.chosen = None;
            return;
        }
        row = (row + down).clamp(0, held - 1);
        self.chosen = Some((lane, row as usize));
    }

    /// Put the ring back on a card that exists, or take it off the board.
    ///
    /// Called from `refresh`, so every change to the board goes through it: a ticket deleted, a card moved to
    /// another lane, a search typed. The row is clamped to the last one the lane has rather than dropped, because
    /// somebody who deletes the card they were on means to carry on from there.
    fn keep_the_ring_on_a_card(&mut self) {
        let Some((lane, row)) = self.chosen else {
            return;
        };
        let counts = self.lane_counts();
        let held = counts.iter().find(|(one, _)| *one == lane).map(|(_, held)| *held).unwrap_or(0);
        self.chosen = match held {
            0 => match counts.iter().find(|(_, held)| *held > 0) {
                // The lane emptied, so the ring moves to the first lane that holds anything rather than going out
                // altogether: a board with cards on it should always be one Enter away from opening one.
                Some((lane, _)) => Some((*lane, 0)),
                None => None,
            },
            held => Some((lane, row.min(held - 1))),
        };
    }

    /// Which ticket the ring is on, if it is on one.
    pub fn the_chosen_ticket(&self, counts: &[(Status, usize)]) -> Option<i64> {
        let (lane, row) = self.chosen?;
        let held = counts.iter().find(|(one, _)| *one == lane).map(|(_, held)| *held)?;
        if row >= held {
            return None;
        }
        // Filtered by the search, the same way the lane is drawn. Reading the unfiltered lane would put the ring
        // on the card at that row and open a different ticket, because a search hides cards without removing them.
        self.board
            .lane(lane)
            .and_then(|found| {
                found
                    .tasks
                    .iter()
                    .filter(|task| board::matches(task, &self.query))
                    .nth(row)
            })
            .map(|task| task.id)
    }

    /// Give the keyboard to the ticket's terminal, or take it back.
    ///
    /// One flag rather than a guess per frame, because the board has fields of its own — a title, a comment
    /// box — and a keystroke belongs to exactly one of them.
    pub fn focus_the_terminal(&mut self, focused: bool) {
        self.terminal_focused = focused;
    }

    /// Tick or untick one todo and read the ticket again.
    pub fn set_todo(&mut self, id: i64, done: bool) -> Result<(), String> {
        let now = clock::now();
        self.store()?.set_todo_done(id, done, &now)?;
        self.refresh()
    }

    /// Move a card to a lane and a place in it, from a drag or from the command line.
    ///
    /// One move does one more thing: **a card sent back from Agent Done to QA Failed resumes its session**,
    /// because a person rejecting finished work wants to tell the agent why, and the agent then decides
    /// whether to claim the ticket again — which is what moves it to In Progress. That is the board being
    /// replaced's own rule, and it is here rather than in the drag so that the command line does it too.
    pub fn move_card(&mut self, id: i64, status: Status, position: i64) -> Result<(), String> {
        let now = clock::now();
        let was = self.store()?.task(id)?;
        self.store()?.move_task(id, status, position, &now)?;
        self.refresh()?;
        let sent_back = was
            .as_ref()
            .is_some_and(|task| task.status == Status::AgentDone && status == Status::QaFailed);
        self.message.clear();
        if sent_back {
            if let Some(task) = was {
                // A resume that cannot happen — a Codex ticket, or one whose agent has never run — says so
                // in the status bar rather than failing the move. The move is what was asked for.
                match self.resume(&task.key) {
                    Ok(answer) => self.message = answer.message,
                    Err(problem) => self.message = problem,
                }
            }
        }
        Ok(())
    }

    /// The tickets one of the listing views shows, as they were last read.
    ///
    /// **Read on a refresh rather than while drawing.** Querying SQLite inside the draw is the one thing the
    /// design says the board never does, and it was doing it twice: once a frame for Backlog and once for
    /// Completed. They are read when the view is chosen and when a command changes something, which is when
    /// they can have changed.

    pub fn listing(&self, view: View) -> &[Task] {
        match view {
            View::Backlog => &self.backlog,
            View::Completed => &self.completed,
            _ => &[],
        }
    }

    /// How far the lanes have been scrolled sideways, taking this frame's scroll into account.
    ///
    /// The lanes are wider than a 420 point pane, so the board scrolls rather than squeezing a lane too
    /// thin for a card's title. Held on the provider rather than in egui's memory because the pane and the
    /// tab are two places drawing the same board and both should be where they were left.
    pub fn scroll_the_lanes(&mut self, ui: &egui::Ui, area: egui::Rect, most: f32) -> f32 {
        if most <= 0.0 {
            self.lane_scroll = 0.0;
            return 0.0;
        }
        let over = ui.ctx().pointer_interact_pos().is_some_and(|at| area.contains(at));
        if over {
            let wheel = ui.ctx().input(|input| {
                // A trackpad gives a sideways delta of its own; a wheel gives a vertical one.
                match input.smooth_scroll_delta.x.abs() > 0.5 {
                    true => input.smooth_scroll_delta.x,
                    // **A vertical wheel is left to the lane under the pointer.** Both this and
                    // `scroll_a_lane` used to read the same vertical delta, so one gesture over a long lane
                    // moved the cards down and slid the whole row of lanes sideways at the same time. A lane
                    // scrolls down; the board scrolls sideways; and the only gesture that means sideways is a
                    // sideways one. What is left for a board with no trackpad is the bar along the bottom and
                    // the arrow keys, both of which move it.
                    false => 0.0,
                }
            });
            self.lane_scroll -= wheel;
        }
        self.lane_scroll = self.lane_scroll.clamp(0.0, most);
        self.lane_scroll
    }

    /// Bring a lane into view sideways, which is what an arrow key crossing to an off screen lane needs.
    ///
    /// The board is a row of four lanes and a pane 420 points wide holds one and a half of them, so the ring
    /// moving right had to be able to move the board under it. Without this, the ring could only reach a lane
    /// that happened to be showing, and pressing right on the last visible one did nothing.
    pub fn show_the_lane(&mut self, lane: Status, lane_width: f32, gap: f32, room: f32, most: f32) {
        let index = Status::ALL.iter().position(|one| *one == lane).unwrap_or(0) as f32;
        let left = index * (lane_width + gap);
        let right = left + lane_width;
        // Scrolled only as far as it takes, and in whichever direction is needed: a lane off the left edge comes
        // to the left edge, one off the right comes to the right edge, and one already showing does not move.
        if left < self.lane_scroll {
            self.lane_scroll = left;
        } else if right > self.lane_scroll + room {
            self.lane_scroll = right - room;
        }
        self.lane_scroll = self.lane_scroll.clamp(0.0, most.max(0.0));
    }

    /// How many cards each lane holds after the search has narrowed it, which is what the ring moves over.
    ///
    /// All four lanes, not the ones on screen. Counting only what was drawn is what stopped the ring reaching a
    /// lane the board had scrolled past.
    pub fn lane_counts(&self) -> Vec<(Status, usize)> {
        Status::ALL
            .into_iter()
            .map(|status| {
                let held = self
                    .board
                    .lane(status)
                    .map(|lane| lane.tasks.iter().filter(|task| board::matches(task, &self.query)).count())
                    .unwrap_or(0);
                (status, held)
            })
            .collect()
    }

    /// How far one lane has been scrolled down, taking this frame's scroll into account.
    ///
    /// Per lane rather than one number for the board, because the lanes hold different numbers of cards and a
    /// shared number would scroll an empty lane as far as a full one. Held on the provider for the reason the
    /// sideways scroll is: the pane and the tab are two places drawing the same board.
    pub fn scroll_a_lane(
        &mut self,
        ui: &egui::Ui,
        lane: Status,
        area: egui::Rect,
        most: f32,
    ) -> f32 {
        let at = self.lane_downs.iter().position(|(known, _)| *known == lane);
        let mut down = at.map(|at| self.lane_downs[at].1).unwrap_or(0.0);
        if most <= 0.0 {
            down = 0.0;
        } else if ui.ctx().pointer_interact_pos().is_some_and(|pointer| area.contains(pointer)) {
            // A lane is a column of cards, so a wheel means further down it. The sideways scroll of the board
            // as a whole reads the horizontal delta, so the two do not fight over one gesture.
            down -= ui.ctx().input(|input| input.smooth_scroll_delta.y);
        }
        down = down.clamp(0.0, most);
        match at {
            Some(at) => self.lane_downs[at].1 = down,
            None => self.lane_downs.push((lane, down)),
        }
        down
    }

    /// How far the Backlog or Completed listing is scrolled down.
    ///
    /// One number for the two, because one of them is showing at a time.
    pub fn listing_scroll(&mut self, ui: &egui::Ui, area: egui::Rect, most: f32) -> f32 {
        if most <= 0.0 {
            self.listing_down = 0.0;
            return 0.0;
        }
        if ui.ctx().pointer_interact_pos().is_some_and(|pointer| area.contains(pointer)) {
            self.listing_down -= ui.ctx().input(|input| input.smooth_scroll_delta.y);
        }
        self.listing_down = self.listing_down.clamp(0.0, most);
        self.listing_down
    }

    /// Pump every terminal, and say whether any of them moved.
    ///
    /// Called once a frame while the board is open and once per watchdog tick. Moved means printed something or
    /// still has a line waiting for its agent's prompt: those are the two reasons the window has to draw again
    /// on its own rather than waiting for the session's waker.
    pub fn pump(&mut self) -> bool {
        let now = clock::now();
        let mut moved = false;
        for terminal in &mut self.terminals {
            moved |= terminal.pump(&now);
        }
        moved
    }

    /// How the terminals ask the window to draw again. Set once, when the provider is opened.
    pub fn set_waker(&mut self, waker: quill_terminal::Waker) {
        self.waker = Some(waker);
    }

    /// Every terminal this board has open.
    ///
    /// What a test waiting on an agent reads to tell a running agent from one that has exited, and to print the
    /// screen when it fails. The window itself always asks about one ticket, which is `terminal_for`.
    pub fn terminals(&self) -> &[TicketTerminal] {
        &self.terminals
    }

    pub fn terminal_for(&self, task_id: i64) -> Option<&TicketTerminal> {
        self.terminals.iter().find(|terminal| terminal.task_id == task_id)
    }

    pub fn terminal_for_mut(&mut self, task_id: i64) -> Option<&mut TicketTerminal> {
        self.terminals.iter_mut().find(|terminal| terminal.task_id == task_id)
    }

    /// What the watchdog sees of one ticket's terminal.
    pub fn terminal_state(&self, task_id: i64, now: &str) -> watchdog::Terminal {
        match self.terminal_for(task_id) {
            None => watchdog::Terminal::Gone,
            Some(terminal) if terminal.paused => watchdog::Terminal::Paused,
            Some(terminal) if !terminal.session.is_running() => watchdog::Terminal::Gone,
            Some(terminal) => watchdog::Terminal::Alive {
                silent_minutes: clock::minutes_between(&terminal.last_output_at, now),
            },
        }
    }

    /// One watchdog tick. Called every two minutes by the window, and by a test with a fixed instant.
    ///
    /// The decisions come from `watchdog::decide`, which has no clock and no database, so this function
    /// is only the two things that need one: reading the candidates, and acting on what was decided.
    pub fn watchdog_tick(&mut self, now: &str) -> Result<Vec<(String, watchdog::Decision)>, String> {
        let thresholds = watchdog::Thresholds {
            lease_minutes: self.configuration.lease_minutes,
            ..watchdog::Thresholds::default()
        };
        let candidates = self.store()?.watchdog_candidates(now, thresholds.lease_minutes)?;
        let mut acted = Vec::new();
        for card in candidates {
            // A card another window owns is that window's business. Without this, a window with no terminal
            // for a card reads it as a card whose worker is gone, and once the lease expires it strikes and
            // reclaims work that is still running somewhere else. A card whose owning window has gone is
            // nobody's, so it is picked up here.
            let owned_elsewhere = match self.store()?.owner_of(card.id)? {
                Some(recorded) => recorded != owner() && !owner_is_gone(Some(&recorded)),
                None => false,
            };
            if owned_elsewhere {
                continue;
            }
            let terminal = self.terminal_state(card.id, now);
            let decision = watchdog::decide(&card, terminal, thresholds, false);
            match &decision {
                watchdog::Decision::Leave => continue,
                watchdog::Decision::ClearCounters => {
                    self.store()?.heartbeat(card.id, None, now)?;
                }
                watchdog::Decision::Strike { first } => {
                    let strikes = self.store()?.strike(card.id)?;
                    if *first {
                        let said = watchdog::strike_comment(
                            &card.key,
                            strikes,
                            thresholds.strikes_before_reclaim,
                        );
                        self.store()?.add_comment(card.id, Author::System, &said, now)?;
                    }
                }
                watchdog::Decision::Reclaim => {
                    let said = watchdog::reclaim_comment(&card.key);
                    self.store()?.add_comment(card.id, Author::System, &said, now)?;
                    self.store()?.reclaim(card.id, now)?;
                    self.terminals.retain(|terminal| terminal.task_id != card.id);
                }
                watchdog::Decision::Nudge { instruction, .. } => {
                    let line = instruction.clone();
                    self.store()?.nudge(card.id, now)?;
                    if let Some(terminal) = self.terminal_for_mut(card.id) {
                        terminal.type_line(&line);
                    }
                }
            }
            acted.push((card.key.clone(), decision));
        }
        if !acted.is_empty() {
            self.refresh()?;
        }
        Ok(acted)
    }
}

/// One field of a ticket, as the modal's controls report it.
///
/// A value per control rather than a `TaskEdit` built by each of them, so the checking — that an assignee is
/// one of three, that an effort is one of five, that an epic is a number — happens in one place and says the
/// same thing however it was asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Assignee(String),
    Priority(String),
    Model(String),
    Effort(String),
    Project(String),
    Epic(String),
    /// The JIRA issue key, typed into the JIRA panel on the ticket.
    JiraKey(String),
}

/// The effort levels a ticket may name.
///
/// Claude's five. Codex knows nothing above `high` and `agent::codex_effort` is what collapses the top two,
/// so the list here is the wider of the two and the narrowing happens where the command line is built.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// How many lines of a terminal are read to tell whether it printed anything.
///
/// One screenful. Reading the whole scrollback once a frame per ticket is what this number exists to avoid:
/// on a busy agent that is megabytes rebuilt sixty times a second, and the question is only whether anything
/// moved.
const TAIL_LINES: usize = 60;

/// Which window owns a claim: this process.
///
/// Two Quill windows can have one board file open, and each runs its own watchdog over the same rows. A
/// window that has no terminal for a card would otherwise read it as a card whose worker is gone, and after
/// the lease expired it would strike and reclaim work still running in the other window. The process id is
/// enough to tell the two apart, and [`owner_is_gone`] is what asks whether the window that took a claim is
/// still running.
fn owner() -> String {
    format!("pid:{}", std::process::id())
}

/// True when the window that took this claim is no longer running.
///
/// A card owned by a window that has gone is a card whose worker is gone, whoever asks. `kill(pid, 0)` is
/// the question the operating system answers: it sends no signal and says whether the process is there.
/// An owner this function cannot read at all is treated as gone, because a card nobody can account for
/// should be recoverable rather than stuck.
fn owner_is_gone(recorded: Option<&str>) -> bool {
    let Some(pid) = recorded.and_then(|owner| owner.strip_prefix("pid:")) else {
        return true;
    };
    let Ok(pid) = pid.parse::<i32>() else {
        return true;
    };
    if pid == std::process::id() as i32 {
        return false;
    }
    // Safety: `kill` with signal 0 sends nothing. It reports whether a process with this id exists and
    // whether this user may signal it, which is exactly the question, and it cannot affect the process.
    unsafe { libc::kill(pid, 0) != 0 }
}

/// A conversation id: a hyphenated hexadecimal string of the shape both agents accept.
///
/// Built from the clock and the address of a local value rather than from a random number generator,
/// because Quill has none and adding one to name a session would be a dependency for a string. Two ids
/// made in the same nanosecond by the same process would collide; the claim is a guarded update, so a
/// collision is refused rather than silently sharing a conversation.
fn new_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);
    let here = &nanos as *const u128 as usize;
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (nanos >> 64) as u32 ^ here as u32,
        (nanos >> 48) as u16,
        (nanos >> 32) as u16 & 0x0fff,
        (nanos >> 16) as u16 & 0x0fff,
        nanos as u64 & 0xffff_ffff_ffff
    )
}

impl UiProvider for AgentTasks {
    fn id(&self) -> &'static str {
        "agent-tasks"
    }

    fn open(&mut self, context: &Context) -> Result<(), String> {
        self.project = context.project.clone();
        self.recent_projects = context.recent_projects.clone();
        // How a terminal asks for a frame. Without one, a terminal that printed while nobody was pointing at
        // the window would wait for the pointer to move, and asking for a frame on a timer instead would keep
        // the graphics card busy while an agent sits at its prompt.
        if let Some(wake) = context.wake.clone() {
            self.waker = Some(wake);
        }
        // **No folder means a board in memory**, and that is what stops a test reading or writing the
        // board somebody is using. The released binary always has one, because `QuillApp::load_settings`
        // is what hands it over; a window a test builds has none, exactly as it has no settings file and
        // no `.quill` folder. The rule is `QuillApp::load_settings`' own, kept once more.
        match context.folder.clone() {
            Some(folder) => {
                self.configuration = Configuration::read(&folder);
                self.folder = Some(folder);
                self.store = Some(Store::open(self.configuration.database_path())?);
            }
            None => {
                self.configuration = Configuration::default();
                self.folder = None;
                self.store = Some(Store::in_memory()?);
            }
        }
        self.refresh()
    }

    fn is_open(&self) -> bool {
        self.store.is_some()
    }

    fn pane(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::agent_tasks::pane(self, ui, look)
    }

    fn tab(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::agent_tasks::tab(self, ui, look)
    }

    fn settings(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::agent_tasks::settings(self, ui, look)
    }

    fn command(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String> {
        let argument = |index: usize| arguments.get(index).map(String::as_str).unwrap_or_default();
        // Everything from `index` on, joined by spaces, which is what a text argument is: `todo-add
        // task-27 Read the old importer` carries the text as the rest of the line. Empty when there is
        // nothing there — `arguments[index..]` would **panic** on a short line, and a command line that
        // takes the window down is worse than one that says what it needed.
        let rest = |index: usize| match arguments.len() > index {
            true => arguments[index..].join(" "),
            false => String::new(),
        };
        let now = clock::now();
        match command {
            "board" => {
                // `board` means the lanes, so it closes whatever ticket was open. Without that, creating a
                // ticket and then asking for the board would answer with the ticket that was just opened,
                // and the pane would still be showing it.
                self.close_detail();
                self.view = View::Board;
                self.refresh()?;
                Ok(Answer::nothing().with(UiProvider::view(self)))
            }
            "back" => {
                self.modal_open = false;
                self.close_detail();
                Ok(Answer::said("showing the lanes"))
            }
            // Opening and closing the modal, so everything a person can do to it an agent can do too.
            "open" => {
                let task = self.by_key(argument(0))?;
                self.open_the_modal(task.id)?;
                Ok(Answer::said(format!("{} is open", task.key)).with(self.detail_json()))
            }
            "close" => {
                self.modal_open = false;
                self.detail.is_new = false;
                Ok(Answer::said("the ticket is closed"))
            }
            "reload" => {
                self.refresh()?;
                Ok(Answer::said("the board was read again"))
            }
            // `task-28`: "Clear out existing tasks. They were cloned and are out of date."
            //
            // A command rather than a button. Emptying a board is not something to put one press away from
            // somebody's hand, and the ticket asks for it once. Two safeguards, because this is the one thing on
            // this board that destroys work: the file is copied first, and the word `confirm` is required.
            "clear" => {
                let store = self.store()?;
                let board = store.board()?;
                let (tickets, todos, comments) = (
                    board.total(),
                    board.lanes.iter().map(|lane| lane.tasks.iter().map(|card| card.todo_count).sum::<i64>()).sum::<i64>(),
                    board.lanes.iter().map(|lane| lane.tasks.iter().map(|card| card.comment_count).sum::<i64>()).sum::<i64>(),
                );
                if argument(0) != "confirm" {
                    return Ok(Answer::said(format!(
                        "this would delete {tickets} tickets with their todos and comments, and leave the epics \
                         and the sprints. Nothing has been deleted: run `clear confirm` to do it."
                    ))
                    .with(json!({
                        "would_delete": { "tickets": tickets, "todos": todos, "comments": comments },
                        "deleted": false,
                    })));
                }
                // Copied first, and named for when it was made, so two clears do not overwrite one another.
                let file = self.configuration.database_path();
                let stamp = clock::now().replace([':', '.'], "-");
                let copy = file.with_file_name(format!("board-before-clear-{stamp}.sqlite3"));
                let copied = self.store()?.copy_the_file(&copy)?;
                let (tickets, todos, comments) = self.store()?.clear_the_tickets()?;
                self.close_detail();
                self.refresh()?;
                self.message = format!(
                    "{tickets} tickets deleted, with {todos} todos and {comments} comments. The board as it was \
                     is in {}",
                    copied.display()
                );
                Ok(Answer::said(self.message.clone()).with(json!({
                    "deleted": true,
                    "tickets": tickets,
                    "todos": todos,
                    "comments": comments,
                    "backup": copied.display().to_string(),
                })))
            }
            // `task-28`: the same change the two buttons on a description and on each comment make, reached the
            // same way by an agent — one function behind both, which is Quill's own rule about a control.
            "show" => {
                let how = |value: &str| match value {
                    "markdown" | "rendered" => Ok(true),
                    "raw" | "source" => Ok(false),
                    other => Err(format!(
                        "`{other}` is not a way to read this: say `markdown` or `raw`"
                    )),
                };
                match argument(0) {
                    "description" => {
                        let rendered = how(argument(1))?;
                        self.show_the_description_rendered(rendered);
                        Ok(Answer::said(match rendered {
                            true => "the description is shown as markdown",
                            false => "the description is shown as its source",
                        }))
                    }
                    "comment" => {
                        let id: i64 = argument(1)
                            .parse()
                            .map_err(|_| format!("`{}` is not a comment id", argument(1)))?;
                        if !self.detail.comments.iter().any(|comment| comment.id == id) {
                            return Err(format!(
                                "the ticket that is open has no comment {id}: its comments are {}",
                                match self.detail.comments.is_empty() {
                                    true => "none".to_owned(),
                                    false => self
                                        .detail
                                        .comments
                                        .iter()
                                        .map(|comment| comment.id.to_string())
                                        .collect::<Vec<String>>()
                                        .join(", "),
                                }
                            ));
                        }
                        let rendered = how(argument(2))?;
                        self.show_the_comment_raw(id, !rendered);
                        Ok(Answer::said(match rendered {
                            true => format!("comment {id} is shown as markdown"),
                            false => format!("comment {id} is shown as its source"),
                        }))
                    }
                    other => Err(format!(
                        "`{other}` cannot be shown one way or the other: say `description` or `comment <id>`"
                    )),
                }
            }
            // One command that is about the window rather than about the board. The provider cannot open a
            // tab itself — only the window can — so it answers and `QuillApp` acts, which is the rule every
            // request in `plugin_ui::Request` follows.
            //
            // `open-pane` was the other one and it is gone with the pane: `task-28` asked for the board to be
            // a tab and nothing else, and the manifest no longer contributes a pane for it to show.
            "open-tab" => Ok(Answer::said("opening the board in a tab")),
            "view" => {
                let view = View::parse(argument(0)).ok_or_else(|| {
                    format!(
                        "there is no `{}` view: this board shows {}",
                        argument(0),
                        View::ALL.iter().map(|view| view.name()).collect::<Vec<&str>>().join(", ")
                    )
                })?;
                self.view = view;
                Ok(Answer::said(format!("showing {}", view.label())))
            }
            "task" => {
                let task = self.by_key(argument(0))?;
                self.open_detail(task.id)?;
                Ok(Answer::nothing().with(self.detail_json()))
            }
            "new-task" => {
                let draft = NewTask {
                    title: arguments.join(" "),
                    assignee: self.configuration.agent,
                    sprint_id: self.board.sprint.as_ref().map(|sprint| sprint.id),
                    project: self
                        .configuration
                        .project
                        .as_ref()
                        .map(|folder| folder.display().to_string()),
                    ..NewTask::default()
                };
                let task = self.store()?.create_task(draft, &now)?;
                self.refresh()?;
                // Opened as a new one, which is what puts the six fields and the description in front of
                // somebody: a ticket that cannot be given an assignee, a model and a project is a ticket that
                // cannot be started.
                self.open_a_new_ticket(task.id)?;
                Ok(Answer::said(format!("{} created", task.key))
                    .with(json!({"task": task.key, "id": task.id})))
            }
            "edit-task" => {
                let task = self.by_key(argument(0))?;
                let edit = TaskEdit {
                    title: (!argument(1).is_empty()).then(|| rest(1)),
                    ..TaskEdit::default()
                };
                self.store()?.edit_task(task.id, &edit, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{} changed", task.key)))
            }
            "move-task" => {
                let task = self.by_key(argument(0))?;
                let status = Status::parse(argument(1)).ok_or_else(|| {
                    format!(
                        "there is no `{}` lane: the board has {}",
                        argument(1),
                        Status::ALL.iter().map(|status| status.name()).collect::<Vec<&str>>().join(", ")
                    )
                })?;
                let position = argument(2).parse::<i64>().unwrap_or(i64::MAX);
                // Through `move_card` rather than straight at the store, so the command line and a drag do the
                // same thing — including the one extra thing a move does, which is hand a ticket sent back from
                // Agent Done to its agent.
                self.move_card(task.id, status, position)?;
                let said = match self.message.is_empty() {
                    true => format!("{} moved to {}", task.key, status.label()),
                    // What the resume said, which is the more interesting half when there is one.
                    false => format!("{} moved to {}. {}", task.key, status.label(), self.message),
                };
                Ok(Answer::said(said))
            }
            "delete-task" => {
                let task = self.by_key(argument(0))?;
                self.store()?.delete_task(task.id)?;
                if self.detail.task.as_ref().map(|open| open.id) == Some(task.id) {
                    self.close_detail();
                }
                self.refresh()?;
                Ok(Answer::said(format!("{} deleted", task.key)))
            }
            "priority" => {
                let task = self.by_key(argument(0))?;
                let priority = Priority::parse(argument(1)).ok_or_else(|| {
                    format!("there is no `{}` priority: low, medium or high", argument(1))
                })?;
                let edit = TaskEdit { priority: Some(priority), ..TaskEdit::default() };
                self.store()?.edit_task(task.id, &edit, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{} is {}", task.key, priority.name())))
            }
            "assign" => {
                let task = self.by_key(argument(0))?;
                let assignee = Assignee::parse(argument(1)).ok_or_else(|| {
                    format!("there is no `{}` assignee: claude, codex or human", argument(1))
                })?;
                let edit = TaskEdit { assignee: Some(assignee), ..TaskEdit::default() };
                self.store()?.edit_task(task.id, &edit, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{} is {}'s", task.key, assignee.name())))
            }
            "todo-add" => {
                let task = self.by_key(argument(0))?;
                let text = rest(1);
                if text.trim().is_empty() {
                    return Err("a todo with no text would be a row nobody can act on".to_owned());
                }
                let todo = self.store()?.add_todo(task.id, &text, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("todo added to {}", task.key))
                    .with(json!({"id": todo.id, "text": todo.text})))
            }
            "todo-done" | "todo-undone" => {
                let task = self.by_key(argument(0))?;
                let which = argument(1).parse::<usize>().unwrap_or(0);
                let todos = self.store()?.todos(task.id)?;
                let todo = todos
                    .get(which.saturating_sub(1))
                    .ok_or_else(|| format!("{} has no todo {which}", task.key))?;
                self.store()?.set_todo_done(todo.id, command == "todo-done", &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{}: {}", task.key, todo.text)))
            }
            "todo-remove" => {
                let task = self.by_key(argument(0))?;
                let which = argument(1).parse::<usize>().unwrap_or(0);
                let todos = self.store()?.todos(task.id)?;
                let todo = todos
                    .get(which.saturating_sub(1))
                    .ok_or_else(|| format!("{} has no todo {which}", task.key))?;
                self.store()?.delete_todo(todo.id)?;
                self.refresh()?;
                Ok(Answer::said(format!("todo removed from {}", task.key)))
            }
            "comment" => {
                let task = self.by_key(argument(0))?;
                let body = rest(1);
                if body.trim().is_empty() {
                    return Err("a comment with no body says nothing".to_owned());
                }
                self.store()?.add_comment(task.id, Author::Human, &body, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("commented on {}", task.key)))
            }
            "jira-key" => {
                let task = self.by_key(argument(0))?;
                self.edit_field(task.id, Field::JiraKey(rest(1)))?;
                Ok(Answer::said(format!("{} names its JIRA issue", task.key)))
            }
            "comment-edit" => {
                let id: i64 = argument(0)
                    .parse()
                    .map_err(|_| format!("`{}` is not a comment id", argument(0)))?;
                let body = rest(1);
                if body.trim().is_empty() {
                    return Err("a comment with no body says nothing".to_owned());
                }
                let changed = self.store()?.edit_comment(id, &body, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("comment {} changed", changed.id)))
            }
            "comment-send" => {
                let task = self.by_key(argument(0))?;
                let body = rest(1);
                if body.trim().is_empty() {
                    return Err("a comment with no body says nothing".to_owned());
                }
                // Sent first, recorded second. The other order left a comment on the board that looked sent
                // when the agent could not be reached, and a comment that looks sent is worse than one that
                // was not written: somebody waits for an answer to a question nobody was asked.
                let sent = self.send(argument(0), &agent::comment_handoff(&task.key, &body));
                match sent {
                    Ok(answer) => {
                        self.store()?.add_comment(task.id, Author::Human, &body, &now)?;
                        self.refresh()?;
                        Ok(answer)
                    }
                    Err(problem) => Err(format!(
                        "{problem} The comment was not posted either, so nothing on the board says it was \
                         sent. Use `comment` to post it without sending."
                    )),
                }
            }
            "heartbeat" => {
                let task = self.by_key(argument(0))?;
                let minutes = argument(1).parse::<i64>().ok();
                self.store()?.heartbeat(task.id, minutes, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{} heard from", task.key)))
            }
            "start" => self.start(argument(0)),
            "resume" => self.resume(argument(0)),
            "send" => {
                let line = rest(1);
                self.send(argument(0), &line)
            }
            "interrupt" => {
                let task = self.by_key(argument(0))?;
                let terminal = self
                    .terminal_for_mut(task.id)
                    .ok_or_else(|| format!("{} has no terminal", task.key))?;
                terminal.session.interrupt();
                Ok(Answer::said(format!("interrupted {}", task.key)))
            }
            "stop" => {
                let task = self.by_key(argument(0))?;
                let had = self.terminals.iter().any(|terminal| terminal.task_id == task.id);
                self.terminals.retain(|terminal| terminal.task_id != task.id);
                match had {
                    true => Ok(Answer::said(format!("{}'s terminal was closed", task.key))),
                    false => Err(format!("{} has no terminal", task.key)),
                }
            }
            "search" => {
                let query = arguments.join(" ");
                self.search(&query)?;
                Ok(Answer::nothing().with(json!({
                    "query": query,
                    "found": self.results.iter().map(|task| task.key.clone()).collect::<Vec<String>>(),
                })))
            }
            // Both of these are on the plugin's `New` submenu, and a menu entry passes no arguments — so with a
            // required name they were two controls that could only fail. They name themselves instead, from what
            // is already there, and the name can be changed afterwards like any other.
            "new-epic" => {
                let name = match arguments.join(" ").trim() {
                    "" => format!("Epic {}", self.store()?.epics()?.len() + 1),
                    said => said.to_owned(),
                };
                // **No colour.** A plugin does not choose one, which is the rule the palette being closed exists
                // for, so an epic is created with none and the store's own default is what draws until somebody
                // sets one. `#2F6BFF` here was the plugin picking a colour.
                let epic = self.store()?.create_epic(&name, "")?;
                self.refresh()?;
                Ok(Answer::said(format!("{} created", epic.name)))
            }
            "new-sprint" => {
                let name = match arguments.join(" ").trim() {
                    "" => format!("Sprint {}", self.store()?.sprints()?.len() + 1),
                    said => said.to_owned(),
                };
                let sprint = self.store()?.create_sprint(&name, model::SprintStatus::Active, &now)?;
                self.refresh()?;
                Ok(Answer::said(format!("{} is the active sprint", sprint.name)))
            }
            // Named so that `plugins show` lists it and an agent is told plainly rather than being left to
            // find out by trying. There is no menu entry for it, because a control that cannot apply is
            // absent.
            "sync" => Err(
                "this board does not sync with JIRA. That is the one part of the survey the plugin does not \
                 do: reading JIRA is HTTP and Quill has no HTTP client, so it is its own piece of work. The \
                 board's own tickets are all there is here."
                    .to_owned(),
            ),
            // The window's own clock, every two minutes. It is a command so that it goes down the one
            // path a change goes down, and so an agent can ask for a tick by hand rather than waiting.
            "tick" | "watchdog" => {
                // Every terminal is read first, even the ones whose board is not showing, or an agent
                // that printed while the pane was put away would count as silent and be nudged.
                self.pump();
                let acted = self.watchdog_tick(&now)?;
                Ok(Answer::said(format!("{} card(s) acted on", acted.len())).with(json!({
                    "acted": acted
                        .iter()
                        .map(|(key, decision)| json!({"task": key, "decision": format!("{decision:?}")}))
                        .collect::<Vec<serde_json::Value>>(),
                })))
            }
            other => Err(format!(
                "there is no `{other}` command on this board: {}",
                self.commands().iter().map(|(name, _)| *name).collect::<Vec<&str>>().join(", ")
            )),
        }
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("board", "The lanes, their counts and their cards. Closes whatever ticket was open."),
            ("back", "Close the ticket that is open and show the lanes."),
            ("open", "Open a ticket in the modal, with its description, its terminal and all of its fields."),
            ("close", "Close the ticket modal."),
            ("reload", "Read the board again from its file."),
            (
                "clear",
                "Delete every ticket with its todos and comments, leaving the epics and the sprints. Copies the \
                 board file first and says where the copy is. Takes the word `confirm`; without it, it says what \
                 it would delete and deletes nothing.",
            ),
            (
                "show",
                "Read the open ticket's description or one of its comments as markdown or as its source: \
                 `show description markdown`, `show comment 12 raw`.",
            ),
            ("open-tab", "Open the board as a tab in the editing area."),
            ("view", "Show one of board, backlog, completed or epics."),
            ("task", "One ticket, with its todos and its comments."),
            ("new-task", "Create a ticket in New, with the rest of the line as its title."),
            ("edit-task", "Change a ticket's title."),
            ("move-task", "Move a ticket to a lane and a place in it."),
            ("delete-task", "Delete a ticket, its todos and its comments."),
            ("priority", "Set a ticket's priority to low, medium or high."),
            ("assign", "Assign a ticket to claude, codex or human."),
            ("todo-add", "Add a todo to a ticket."),
            ("todo-done", "Tick a ticket's nth todo."),
            ("todo-undone", "Untick a ticket's nth todo."),
            ("todo-remove", "Delete a ticket's nth todo."),
            ("comment", "Post a comment on a ticket."),
            ("jira-key", "Record which JIRA issue a ticket is about. Nothing is fetched."),
            ("comment-edit", "Change what a comment says. A person's own comments only."),
            ("comment-send", "Post a comment and type it into the ticket's agent."),
            ("heartbeat", "Record that a ticket's agent is working, with a lease in minutes."),
            ("start", "Launch the ticket's agent and hand the ticket over."),
            ("resume", "Bring back a retired session without changing the ticket's lane."),
            ("send", "Type a line into a ticket's agent."),
            ("interrupt", "Send Ctrl+C to a ticket's agent."),
            ("stop", "Close a ticket's terminal."),
            ("search", "Tickets whose key, title or description holds the query."),
            ("new-epic", "Create an epic. Names itself when no name is given, which is what the menu entry does."),
            ("new-sprint", "Create a sprint and make it the active one. Names itself when no name is given."),
            ("sync", "Not implemented on this board: says so and does nothing. There is no menu entry for it."),
            ("tick", "Read every terminal and run one watchdog tick. The window does this every two minutes."),
            ("watchdog", "The same as `tick`, named for what it is for."),
        ]
    }

    fn view(&self) -> serde_json::Value {
        json!({
            "open": self.is_open(),
            "database": self.configuration.database_path().display().to_string(),
            "view": self.view.name(),
            "modal": self.modal_open,
            // Which way the open ticket is being read, so an agent can see what a person is looking at rather
            // than only being able to change it. `task-28`.
            "showing": {
                "description": match self.detail.description_rendered {
                    true => "markdown",
                    false => "raw",
                },
                "comments_as_source": self
                    .detail
                    .comments_raw
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<i64>>(),
            },
            "message": self.message,
            "sprint": self.board.sprint.as_ref().map(|sprint| json!({
                "name": sprint.name,
                "status": sprint.status.name(),
            })),
            "total": self.board.total(),
            "lanes": self
                .board
                .lanes
                .iter()
                .map(|lane| json!({
                    "status": lane.status.name(),
                    "label": lane.status.label(),
                    "count": lane.count(),
                    "cards": lane.tasks.iter().map(card_json).collect::<Vec<serde_json::Value>>(),
                }))
                .collect::<Vec<serde_json::Value>>(),
            "epics": self
                .board
                .epics
                .iter()
                .map(|epic| json!({"name": epic.name, "color": epic.color}))
                .collect::<Vec<serde_json::Value>>(),
            "detail": self.detail.task.as_ref().map(|_| self.detail_json()),
            "terminals": self
                .terminals
                .iter()
                .map(|terminal| json!({
                    "task": terminal.task_id,
                    "session": terminal.session_id,
                    "alive": terminal.session.is_running(),
                    "paused": terminal.paused,
                }))
                .collect::<Vec<serde_json::Value>>(),
        })
    }

    fn modal(&mut self, ctx: &egui::Context, look: &Look<'_>) -> (Vec<Request>, bool) {
        if !self.modal_open || self.detail.task.is_none() {
            return (Vec::new(), false);
        }
        let outcome = crate::components::agent_tasks::ticket_modal::show(self, ctx, look);
        if outcome.closed {
            self.modal_open = false;
            self.close_detail();
        }
        (outcome.requests, outcome.closed)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn keyboard(&mut self, has_it: bool) {
        // **Losing the keys clears the terminal's focus, and gaining them does not set it.** The board asks for the
        // keyboard for two different reasons — somebody clicked into a ticket's terminal, and somebody clicked the
        // lanes so the arrow keys reach them — and this cannot tell which. So the terminal's own click is what
        // sets its focus, in `components::agent_tasks::detail`, and what is left here is the half that is true
        // whatever the reason: a board that has lost the keyboard has no terminal typing into it.
        if !has_it {
            self.terminal_focused = false;
        }
    }

    fn catch_up(&mut self) -> bool {
        // Answers whether something is still **moving**, not whether something is running. A terminal that is
        // alive and quiet needs no frame: its own waker asks for one when it prints. Answering yes for a live
        // terminal kept the window drawing for ever while an agent sat at its prompt.
        self.pump()
    }

    fn close(&mut self) {
        // The sessions go with the provider, and each one's agent keeps its conversation, so closing the
        // board loses no work: `Resume session` brings any of them back on the id in the database.
        self.terminals.clear();
        self.store = None;
        self.board = Board::default();
        self.detail = Detail::default();
        self.results.clear();
        self.backlog.clear();
        self.completed.clear();
    }
}

impl AgentTasks {
    fn detail_json(&self) -> serde_json::Value {
        json!({
            "task": self.detail.task.as_ref().map(card_json),
            "description": self.detail.task.as_ref().map(|task| task.description.clone()),
            "todos": self
                .detail
                .todos
                .iter()
                .map(|todo| json!({"text": todo.text, "done": todo.done}))
                .collect::<Vec<serde_json::Value>>(),
            "comments": self
                .detail
                .comments
                .iter()
                .map(|comment| json!({
                    // The id, because `comment-edit` names a comment by it and there was no way to learn one.
                    "id": comment.id,
                    "author": comment.author.name(),
                    "body": comment.body,
                    "at": comment.created_at,
                }))
                .collect::<Vec<serde_json::Value>>(),
        })
    }
}

fn card_json(task: &Task) -> serde_json::Value {
    json!({
        "key": task.key,
        "id": task.id,
        "title": task.display_title(),
        "status": task.status.name(),
        "priority": task.priority.name(),
        "assignee": task.assignee.name(),
        "model": task.model,
        "effort": task.effort,
        "todos": format!("{}/{}", task.todo_done_count, task.todo_count),
        "comments": task.comment_count,
        "session": task.session_id,
        "jira": task.jira_key,
        "strikes": task.watchdog_strikes,
        "nudges": task.watchdog_nudges,
    })
}

#[cfg(test)]
mod tests_task_28 {
    use super::*;

    /// `task-28`: "We don't need a Schedule field."
    ///
    /// What there was, was `View::Schedule`, the fifth board view, listing the rows in the `task_schedule`
    /// table with when each next runs. Nothing on this board ever writes such a row, so it was a view of a
    /// table that is always empty.
    #[test]
    fn the_board_has_four_views_and_schedule_is_not_one_of_them() {
        assert_eq!(View::ALL.len(), 4);
        let names: Vec<&str> = View::ALL.iter().map(|view| view.name()).collect();
        assert_eq!(names, ["board", "backlog", "completed", "epics"]);
        assert!(View::parse("schedule").is_none(), "there is no schedule view to ask for");
        for name in names {
            assert!(View::parse(name).is_some(), "{name} is asked for by its own name");
        }
    }

    /// `task-28`: "Prefill my settings with the base url for Iliad."
    ///
    /// A configuration that has never been written points at Iliad, and so does one read from a settings file
    /// that has no `base-url` line in it — which is every file written by the version before this one.
    #[test]
    fn a_configuration_nobody_has_written_points_at_iliad() {
        assert_eq!(Configuration::default().base_url.as_deref(), Some(ILIAD_URL));
        let folder = std::env::temp_dir().join(format!("quill-board-iliad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        // A file from before this change: it has the other settings and no `base-url`.
        std::fs::write(folder.join("settings.conf"), "agent = claude\nlease = 45\n").expect("an old file");
        assert_eq!(Configuration::read(&folder).base_url.as_deref(), Some(ILIAD_URL));

        // And a file whose line is **present and empty** is one somebody cleared on purpose, which means the
        // agent's own endpoint rather than Iliad's URL handed back again.
        std::fs::write(folder.join("settings.conf"), "agent = claude\nbase-url =\n").expect("a cleared file");
        assert_eq!(Configuration::read(&folder).base_url, None, "cleared means the agent's own");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The old `key-name` and `key-variable` lines are read without complaint and are gone once it is written
    /// again, which is what `Configuration::read` already does with any name it does not know.
    #[test]
    fn a_settings_file_naming_a_key_and_a_variable_is_read_and_written_back_without_them() {
        let folder = std::env::temp_dir().join(format!("quill-board-oldkeys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        std::fs::write(
            folder.join("settings.conf"),
            "agent = codex\nkey-name = something\nkey-variable = SOMETHING_ELSE\nbase-url = https://gateway\n",
        )
        .expect("a file from the version before this one");
        let read = Configuration::read(&folder);
        assert_eq!(read.agent, Assignee::Codex, "the settings it does know are still read");
        assert_eq!(read.base_url.as_deref(), Some("https://gateway"));
        read.write(&folder).expect("written back");
        let text = std::fs::read_to_string(folder.join("settings.conf")).expect("the file");
        assert!(!text.contains("key-name"), "the name of a keychain entry is not a setting any more: {text}");
        assert!(!text.contains("key-variable"), "and neither is a variable name: {text}");
        assert!(text.contains("base-url = https://gateway"), "{text}");
        // No secret is in the file, which is the rule that matters most about it.
        assert!(!text.contains("x-api-key"), "{text}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// What the agent is launched with: every name that matters, and the gateway.
    #[test]
    fn the_key_reaches_the_agent_under_every_name_that_matters() {
        let configuration = Configuration::default();
        let handed = configuration.environment_given(Some("a-key"));
        let names: Vec<&str> = handed.iter().map(|(name, _)| name.as_str()).collect();
        for wanted in KEY_VARIABLES {
            assert!(names.contains(wanted), "{wanted} is set: {names:?}");
        }
        assert_eq!(
            handed.iter().find(|(name, _)| name == "ANTHROPIC_CUSTOM_HEADERS").map(|(_, value)| value.as_str()),
            Some("x-api-key: a-key"),
            "the header the gateway wants, which `~/.zshrc` also sets"
        );
        assert_eq!(
            handed.iter().find(|(name, _)| name == "ANTHROPIC_BASE_URL").map(|(_, value)| value.as_str()),
            Some(ILIAD_URL)
        );
        assert_eq!(
            handed.iter().find(|(name, _)| name == "OPENAI_BASE_URL").map(|(_, value)| value.as_str()),
            Some(ILIAD_URL)
        );

        // No key anywhere is not an error: both agents know how to log in, so nothing about a key is set and
        // the gateway is still named.
        let without = configuration.environment_given(None);
        assert!(
            without.iter().all(|(name, _)| name.ends_with("BASE_URL")),
            "with no key, only the gateway is handed over: {without:?}"
        );

        // And with no gateway either, nothing at all is handed over and the agent uses its own endpoint.
        let bare = Configuration { base_url: None, ..Configuration::default() };
        assert!(bare.environment_given(None).is_empty(), "the agent's own configuration is left alone");
    }

    /// `task-28`: the `Project` field was free text. The dropdown offers this window's folder and the recent
    /// ones, handed over in `plugin_ui::Context`, and keeps whatever the ticket already says.
    #[test]
    fn the_project_dropdown_offers_this_window_and_the_recent_ones() {
        use crate::services::plugin_ui::{Context, UiProvider};
        let mut board = AgentTasks::new();
        board
            .open(&Context {
                project: Some(PathBuf::from("/here/now")),
                recent_projects: vec![PathBuf::from("/older"), PathBuf::from("/here/now")],
                folder: None,
                wake: None,
            })
            .expect("a board with no settings folder opens in memory");

        // This window first, then the recent ones, and a folder listed twice is listed once.
        assert_eq!(board.known_projects(None), vec!["/here/now".to_owned(), "/older".to_owned()]);

        // A ticket naming a folder this window has never opened keeps it, and it is the first choice, for the
        // reason `agent::models_for` keeps a model it does not know.
        assert_eq!(
            board.known_projects(Some("/somewhere/else")),
            vec!["/somewhere/else".to_owned(), "/here/now".to_owned(), "/older".to_owned()]
        );
        // And one it has opened is not listed twice.
        assert_eq!(board.known_projects(Some("/older")), vec!["/older".to_owned(), "/here/now".to_owned()]);
    }

    /// And the table it read is still there, because dropping a table is deleting data.
    #[test]
    fn the_schedule_table_is_still_read_even_though_no_view_draws_it() {
        let store = Store::in_memory().expect("a board in memory");
        assert!(store.schedules().expect("the schedules").is_empty(), "empty, which is the point");
    }
}
