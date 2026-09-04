//! The board's database: one SQLite file, and every query there is.
//!
//! No user interface dependency, so every rule below is a test with a temporary file and no window.
//! `tasks/agent-tasks-plugin-tdd.md` §4 is the design.
//!
//! ## One file, with SQLite compiled in
//!
//! The application being replaced needs a PostgreSQL server on a port, a role and a schema, and its
//! own notes record what that cost: the container on 5432 stored its data on tmpfs, so every restart
//! destroyed the database. A text editor does not ask for a database server. `rusqlite` with the
//! `bundled` feature compiles SQLite's own C into Unluminous, so there is nothing to install, nothing
//! listening and nothing to be running, and the board is one file that can be copied and backed up.
//!
//! ## Three differences from the schema being replaced, each deliberate
//!
//! `SERIAL` becomes `INTEGER PRIMARY KEY AUTOINCREMENT`. `TIMESTAMPTZ` becomes `TEXT` holding an ISO
//! 8601 instant in UTC, because SQLite has no `now()` that returns an instant with an offset — which
//! also means the caller hands in the instant, and that is what makes every rule above this file
//! testable with a fixed clock. `JSONB` becomes `TEXT`.
//!
//! ## Migration is additive and nothing is ever dropped
//!
//! [`Store::open`] runs `CREATE TABLE IF NOT EXISTS` for every table and then adds any column a later
//! version needs, guarded by a read of `pragma_table_info` because SQLite has no
//! `ADD COLUMN IF NOT EXISTS`. That is the shape the application being replaced chose and it is right:
//! a board somebody has been using is not a thing to recreate. A `meta` row records the schema version
//! the file was last written by, and a file from a **newer** Unluminous is refused with a message rather
//! than opened and half understood.
//!
//! ## Every query is a named function here, and there is no SQL anywhere else
//!
//! That is what keeps the drawing free of a query language and what makes the store swappable: if the
//! board ever has to read the browser's API instead of a file, everything above this file is unchanged.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use super::model::{
    Assignee, Author, Board, Comment, Epic, Lane, Priority, Source, Sprint, SprintStatus, Status,
    Task, Todo,
};

/// What this version of Unluminous writes. A file saying more than this is refused.
///
/// Bumped when a column is added, which is the only kind of change [`migrate`] makes. Version 2 added
/// `task.owner`, which [`migrate`] adds to a board that was written at version 1.
pub const SCHEMA_VERSION: i64 = 2;

/// The file name inside the plugin's own folder.
pub const FILE: &str = "board.sqlite3";

/// The board, open.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
    path: PathBuf,
}

impl Store {
    /// Open the board at `path`, creating the file and the schema when there is none.
    ///
    /// The folder is created too, because a plugin's folder does not exist until something writes to
    /// it, and a person who moved the database in Settings named a folder rather than making one.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|problem| format!("{} could not be made: {problem}", folder.display()))?;
        }
        let connection = Connection::open(&path)
            .map_err(|problem| format!("{} could not be opened: {problem}", path.display()))?;
        Self::prepare(connection, path)
    }

    /// A board in memory, which is what a test opens.
    pub fn in_memory() -> Result<Self, String> {
        let connection = Connection::open_in_memory()
            .map_err(|problem| format!("a board in memory could not be opened: {problem}"))?;
        Self::prepare(connection, PathBuf::from(":memory:"))
    }

    fn prepare(connection: Connection, path: PathBuf) -> Result<Self, String> {
        // Foreign keys are off by default in SQLite, and the cascade that deletes a ticket's todos and
        // comments with it is a foreign key. Off, a deleted ticket would leave its rows behind.
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;\n\
                 PRAGMA journal_mode = WAL;\n\
                 PRAGMA busy_timeout = 3000;",
            )
            .map_err(|problem| format!("the board could not be prepared: {problem}"))?;
        let store = Self { connection, path };
        store.create()?;
        store.check_version()?;
        migrate(&store.connection)?;
        // **After the migration, not before it.** The version says which columns the file has, so it can only be
        // written once they are there. Writing it in `check_version` meant a file made at version 1 stayed at
        // version 1 however many columns were added to it afterwards, which made the number say nothing.
        store.record_version()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The default place the board lives: inside the plugin's own folder under the settings folder.
    ///
    /// `~/Library/Application Support/Unluminous/plugins/agent-tasks/board.sqlite3` on macOS, and the
    /// matching folder on the other two platforms, which is what `store::folder_for_this_person`
    /// already decides for the settings themselves.
    pub fn default_path() -> PathBuf {
        crate::services::store::folder_for_this_person()
            .join("plugins")
            .join("agent-tasks")
            .join(FILE)
    }

    fn create(&self) -> Result<(), String> {
        self.connection
            .execute_batch(SCHEMA)
            .map_err(|problem| format!("the board's tables could not be made: {problem}"))
    }

    /// Refuse a file a newer Unluminous wrote.
    ///
    /// Opening it would mean reading columns this version does not know about and writing rows the
    /// newer one cannot read, and the failure would arrive later as a board with cards missing. Saying
    /// so at the moment the file is opened is the honest answer.
    fn check_version(&self) -> Result<(), String> {
        let found: Option<i64> = self
            .connection
            .query_row("SELECT value FROM meta WHERE name = 'schema_version'", [], |row| row.get(0))
            .optional()
            .map_err(|problem| format!("the board's version could not be read: {problem}"))?;
        match found {
            Some(version) if version > SCHEMA_VERSION => Err(format!(
                "this board was written by a newer Unluminous: it is schema {version} and this Unluminous reads {SCHEMA_VERSION}"
            )),
            _ => Ok(()),
        }
    }

    /// Write down which schema this file is at now, which is this Unluminous's, since [`migrate`] has just brought it
    /// there.
    ///
    /// One statement for a file that has the row and one for a file that does not, rather than an upsert, because
    /// `meta` is two columns and a primary key and the two statements say plainly which case is which.
    fn record_version(&self) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO meta (name, value) VALUES ('schema_version', ?1) \
                 ON CONFLICT(name) DO UPDATE SET value = ?1",
                params![SCHEMA_VERSION],
            )
            .map_err(|problem| format!("the board's version could not be written: {problem}"))?;
        Ok(())
    }

    // ------------------------------------------------------------------ reading the board

    /// The whole board: the active sprint, its four lanes in order, and the epics its cards name.
    ///
    /// One query for the cards rather than one per lane, ordered by status and then position, because
    /// four round trips to answer one screen is three more than the question needs. The lanes are then
    /// filled in `Status::ALL` order, so an empty lane is still a lane and the board always has four.
    pub fn board(&self) -> Result<Board, String> {
        let sprint = self.active_sprint()?;
        let tasks = self.tasks_in(sprint.as_ref().map(|sprint| sprint.id))?;
        let lanes = Status::ALL
            .into_iter()
            .map(|status| Lane {
                status,
                tasks: tasks.iter().filter(|task| task.status == status).cloned().collect(),
            })
            .collect();
        Ok(Board { sprint, lanes, epics: self.epics()? })
    }

    fn tasks_in(&self, sprint: Option<i64>) -> Result<Vec<Task>, String> {
        let sql = format!(
            // `?1 IS NULL` means **the tickets that have no sprint**, not every ticket: a board nobody has
            // organised into sprints shows what is on it, and a board with an active sprint shows that
            // sprint. Written as `OR t.sprint_id = ?1` alone it meant every row, so with no active sprint
            // the board showed the whole history.
            "{TASK_COLUMNS} WHERE ((?1 IS NULL AND t.sprint_id IS NULL) OR t.sprint_id = ?1) \
             ORDER BY t.status, t.position, t.id"
        );
        let mut statement = self.prepared(&sql)?;
        let rows = statement
            .query_map(params![sprint], read_task)
            .map_err(|problem| format!("the board could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Task>, _>>()
            .map_err(|problem| format!("a card could not be read: {problem}"))
    }

    /// Every ticket with no sprint, which is the Backlog view.
    pub fn backlog(&self) -> Result<Vec<Task>, String> {
        let sql = format!("{TASK_COLUMNS} WHERE t.sprint_id IS NULL ORDER BY t.position, t.id");
        self.tasks_by(&sql, params![])
    }

    /// Every ticket in a completed sprint, which is the Completed view.
    pub fn completed(&self) -> Result<Vec<Task>, String> {
        let sql = format!(
            "{TASK_COLUMNS} JOIN sprint s ON s.id = t.sprint_id \
             WHERE s.status = 'completed' ORDER BY s.position DESC, t.position, t.id"
        );
        self.tasks_by(&sql, params![])
    }

    fn tasks_by(&self, sql: &str, arguments: &[&dyn rusqlite::ToSql]) -> Result<Vec<Task>, String> {
        let mut statement = self.prepared(sql)?;
        let rows = statement
            .query_map(arguments, read_task)
            .map_err(|problem| format!("the tickets could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Task>, _>>()
            .map_err(|problem| format!("a ticket could not be read: {problem}"))
    }

    pub fn task(&self, id: i64) -> Result<Option<Task>, String> {
        let sql = format!("{TASK_COLUMNS} WHERE t.id = ?1");
        self.prepared(&sql)?
            .query_row(params![id], read_task)
            .optional()
            .map_err(|problem| format!("ticket {id} could not be read: {problem}"))
    }

    /// The ticket called `task-27`, which is how the command line and an agent name one.
    pub fn task_by_key(&self, key: &str) -> Result<Option<Task>, String> {
        let sql = format!("{TASK_COLUMNS} WHERE t.task_key = ?1");
        self.prepared(&sql)?
            .query_row(params![key], read_task)
            .optional()
            .map_err(|problem| format!("{key} could not be read: {problem}"))
    }

    pub fn todos(&self, task: i64) -> Result<Vec<Todo>, String> {
        let mut statement = self.prepared(
            "SELECT id, task_id, text, done, position, created_at FROM task_todo \
             WHERE task_id = ?1 ORDER BY position, id",
        )?;
        let rows = statement
            .query_map(params![task], |row| {
                Ok(Todo {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    text: row.get(2)?,
                    done: row.get::<_, i64>(3)? != 0,
                    position: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|problem| format!("the todos could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Todo>, _>>()
            .map_err(|problem| format!("a todo could not be read: {problem}"))
    }

    /// A ticket's comments, oldest first, which is the order a conversation is read in.
    pub fn comments(&self, task: i64) -> Result<Vec<Comment>, String> {
        let mut statement = self.prepared(
            "SELECT id, task_id, author, body, created_at FROM task_comment \
             WHERE task_id = ?1 ORDER BY created_at, id",
        )?;
        let rows = statement
            .query_map(params![task], |row| {
                let author: String = row.get(2)?;
                Ok(Comment {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    author: known("comment author", &author, Author::parse(&author))?,
                    body: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|problem| format!("the comments could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Comment>, _>>()
            .map_err(|problem| format!("a comment could not be read: {problem}"))
    }

    pub fn epics(&self) -> Result<Vec<Epic>, String> {
        let mut statement =
            self.prepared("SELECT id, name, color, position FROM task_epic ORDER BY position, id")?;
        let rows = statement
            .query_map([], |row| {
                Ok(Epic {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    position: row.get(3)?,
                })
            })
            .map_err(|problem| format!("the epics could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Epic>, _>>()
            .map_err(|problem| format!("an epic could not be read: {problem}"))
    }

    pub fn sprints(&self) -> Result<Vec<Sprint>, String> {
        let mut statement = self
            .prepared("SELECT id, name, status, position, created_at FROM sprint ORDER BY position, id")?;
        let rows = statement
            .query_map([], read_sprint)
            .map_err(|problem| format!("the sprints could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Sprint>, _>>()
            .map_err(|problem| format!("a sprint could not be read: {problem}"))
    }

    /// The sprint the board shows, which is the one that is active.
    ///
    /// `None` with none, and the board then shows every ticket that has no sprint, so a board nobody
    /// has organised into sprints still draws its cards rather than four empty lanes.
    pub fn active_sprint(&self) -> Result<Option<Sprint>, String> {
        self.prepared(
            "SELECT id, name, status, position, created_at FROM sprint \
             WHERE status = 'active' ORDER BY position, id LIMIT 1",
        )?
        .query_row([], read_sprint)
        .optional()
        .map_err(|problem| format!("the active sprint could not be read: {problem}"))
    }

    // ------------------------------------------------------------------ changing the board

    /// Create a ticket and give it the next key.
    ///
    /// The key is `task-<n>` where `n` is one past the highest number any ticket has ever had, taken
    /// from the same transaction as the insert so two tickets created at once cannot share a key.
    pub fn create_task(&self, draft: NewTask, now: &str) -> Result<Task, String> {
        self.in_transaction(|| self.create_task_now(draft, now))
    }

    fn create_task_now(&self, draft: NewTask, now: &str) -> Result<Task, String> {
        let key = self.next_key()?;
        self.connection
            .execute(
                "INSERT INTO task (task_key, title, description, priority, status, assignee, model, \
                 effort, epic_id, sprint_id, position, project, source, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
                params![
                    key,
                    draft.title,
                    draft.description,
                    draft.priority.name(),
                    draft.status.name(),
                    draft.assignee.name(),
                    draft.model,
                    draft.effort,
                    draft.epic_id,
                    draft.sprint_id,
                    self.next_position(draft.status, draft.sprint_id)?,
                    draft.project,
                    Source::Local.name(),
                    now,
                ],
            )
            .map_err(|problem| format!("the ticket could not be created: {problem}"))?;
        let id = self.connection.last_insert_rowid();
        self.task(id)?.ok_or_else(|| "the ticket was created and could not be read back".to_owned())
    }

    fn next_key(&self) -> Result<String, String> {
        // The highest number any key has held, read as a number rather than as text, because `task-9`
        // sorts after `task-10` as a string and the tenth ticket would be given a key that exists.
        let highest: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(MAX(CAST(SUBSTR(task_key, 6) AS INTEGER)), 0) FROM task \
                 WHERE task_key LIKE 'task-%'",
                [],
                |row| row.get(0),
            )
            .map_err(|problem| format!("the next key could not be worked out: {problem}"))?;
        Ok(format!("task-{}", highest + 1))
    }

    fn next_position(&self, status: Status, sprint: Option<i64>) -> Result<i64, String> {
        self.connection
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM task \
                 WHERE status = ?1 AND ((?2 IS NULL AND sprint_id IS NULL) OR sprint_id = ?2)",
                params![status.name(), sprint],
                |row| row.get(0),
            )
            .map_err(|problem| format!("the next position could not be worked out: {problem}"))
    }

    /// Write the fields a person can edit. Every one is optional, so one field is one call.
    pub fn edit_task(&self, id: i64, edit: &TaskEdit, now: &str) -> Result<(), String> {
        let existing = self
            .task(id)?
            .ok_or_else(|| format!("ticket {id} is not on the board"))?;
        self.connection
            .execute(
                "UPDATE task SET title = ?2, description = ?3, priority = ?4, assignee = ?5, \
                 model = ?6, effort = ?7, epic_id = ?8, project = ?9, jira_key = ?11, \
                 updated_at = ?10 WHERE id = ?1",
                params![
                    id,
                    edit.title.clone().unwrap_or(existing.title),
                    edit.description.clone().unwrap_or(existing.description),
                    edit.priority.unwrap_or(existing.priority).name(),
                    edit.assignee.unwrap_or(existing.assignee).name(),
                    edit.model.clone().unwrap_or(existing.model),
                    edit.effort.clone().unwrap_or(existing.effort),
                    edit.epic_id.unwrap_or(existing.epic_id),
                    edit.project.clone().unwrap_or(existing.project),
                    now,
                    edit.jira_key.clone().unwrap_or(existing.jira_key),
                ],
            )
            .map_err(|problem| format!("ticket {id} could not be changed: {problem}"))?;
        Ok(())
    }

    /// Move a ticket to a lane and a place in it, and close the gap it left behind.
    ///
    /// Positions stay contiguous inside a lane, which is what makes a drag land where the pointer says
    /// rather than after however many holes earlier moves left.
    pub fn move_task(&self, id: i64, status: Status, position: i64, now: &str) -> Result<(), String> {
        self.in_transaction(|| self.move_task_now(id, status, position, now))
    }

    fn move_task_now(
        &self,
        id: i64,
        status: Status,
        position: i64,
        now: &str,
    ) -> Result<(), String> {
        let task = self.task(id)?.ok_or_else(|| format!("ticket {id} is not on the board"))?;
        self.connection
            .execute(
                "UPDATE task SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, status.name(), now],
            )
            .map_err(|problem| format!("ticket {id} could not be moved: {problem}"))?;
        // The card is put at the index that was asked for rather than being given that number and left
        // to a sort. Two cards claiming position 0 is a tie, and a tie broken by anything but the drop
        // is a card that lands one place from where the pointer let go.
        let mut order = self.lane_order(status, task.sprint_id)?;
        order.retain(|known| *known != id);
        let at = (position.max(0) as usize).min(order.len());
        order.insert(at, id);
        self.write_positions(&order)?;
        if status != task.status {
            let left_behind = self.lane_order(task.status, task.sprint_id)?;
            self.write_positions(&left_behind)?;
        }
        Ok(())
    }

    /// The ids in one lane, in the order they are drawn.
    fn lane_order(&self, status: Status, sprint: Option<i64>) -> Result<Vec<i64>, String> {
        let mut statement = self.prepared(
            "SELECT id FROM task WHERE status = ?1 AND ((?2 IS NULL AND sprint_id IS NULL) OR sprint_id = ?2) \
             ORDER BY position, id",
        )?;
        let ids: Vec<i64> = statement
            .query_map(params![status.name(), sprint], |row| row.get(0))
            .map_err(|problem| format!("the lane could not be read: {problem}"))?
            .collect::<Result<Vec<i64>, _>>()
            .map_err(|problem| format!("the lane could not be read: {problem}"))?;
        Ok(ids)
    }

    /// Write 0, 1, 2 down a lane, so its positions are contiguous and hold the order given.
    ///
    /// Contiguous is what makes a drop land where the pointer says: a lane whose positions were 0, 4, 9
    /// would take a drop at index 1 and draw it third.
    fn write_positions(&self, order: &[i64]) -> Result<(), String> {
        for (position, id) in order.iter().enumerate() {
            self.connection
                .execute(
                    "UPDATE task SET position = ?2 WHERE id = ?1",
                    params![id, position as i64],
                )
                .map_err(|problem| format!("the lane could not be renumbered: {problem}"))?;
        }
        Ok(())
    }

    /// Delete every ticket on the board, with its todos and its comments, and answer how many went.
    ///
    /// `task-28`: "Clear out existing tasks. They were cloned and are out of date."
    ///
    /// Epics and sprints are left alone. They are not tickets, the active sprint is what the board draws against,
    /// and a board with no sprint draws "No active sprint" — so taking the sprint would make an empty board look
    /// broken rather than empty.
    ///
    /// The todos and comments go with the tickets through the `ON DELETE CASCADE` on their foreign keys, which is
    /// on because `Store::prepare` turns foreign keys on. The counts are read first so the answer can say what was
    /// deleted, since after the statement there is nothing left to count.
    pub fn clear_the_tickets(&self) -> Result<(i64, i64, i64), String> {
        let count = |table: &str| -> Result<i64, String> {
            self.connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .map_err(|problem| format!("{table} could not be counted: {problem}"))
        };
        let tickets = count("task")?;
        let todos = count("task_todo")?;
        let comments = count("task_comment")?;
        self.connection
            .execute("DELETE FROM task", [])
            .map_err(|problem| format!("the tickets could not be deleted: {problem}"))?;
        Ok((tickets, todos, comments))
    }

    /// Copy the board file, so that something destructive has something to go back to.
    ///
    /// Answers where the copy is. Refused for a board in memory, which has no file to copy and which is every
    /// board a test opens unless it asks for one on disk.
    ///
    /// `VACUUM INTO` rather than `std::fs::copy`, because this board is in write ahead logging mode: the newest
    /// rows may be in `board.sqlite3-wal` rather than in the file itself, so copying the one file can copy a
    /// board that is missing whatever was written most recently. `VACUUM INTO` asks SQLite for a complete copy.
    pub fn copy_the_file(&self, to: &Path) -> Result<PathBuf, String> {
        if self.path == Path::new(":memory:") {
            return Err("this board is in memory, so there is no file to copy".to_owned());
        }
        self.connection
            .execute("VACUUM INTO ?1", params![to.to_string_lossy()])
            .map_err(|problem| format!("{} could not be written: {problem}", to.display()))?;
        Ok(to.to_path_buf())
    }

    /// Take a ticket for an agent, and refuse a second caller.
    ///
    /// One guarded update rather than a read and then a write: two agents pressing start on one card,
    /// or an agent and the board's own button, would both see `new` and both claim it. The row moves
    /// only if it is not already claimed, and the answer says whether this caller is the one that got
    /// it.
    pub fn claim(
        &self,
        id: i64,
        session: &str,
        kind: Assignee,
        owner: &str,
        now: &str,
    ) -> Result<bool, String> {
        let changed = self
            .connection
            .execute(
                "UPDATE task SET status = 'in_progress', assignee = ?3, agent_session_id = ?2, \
                 owner = ?5, heartbeat_at = ?4, watchdog_strikes = 0, watchdog_nudges = 0, \
                 watchdog_nudged_at = NULL, updated_at = ?4 \
                 WHERE id = ?1 AND agent_session_id IS NULL",
                params![id, session, kind.name(), now, owner],
            )
            .map_err(|problem| format!("ticket {id} could not be claimed: {problem}"))?;
        Ok(changed == 1)
    }

    /// Give a claim back, because the agent it was taken for could not be started.
    ///
    /// A ticket that moved to In Progress and then failed to spawn would otherwise sit there with a
    /// session id naming a process that never existed, and the watchdog would strike it three times
    /// before giving it back. The claim and the spawn are two steps and only the caller knows whether the
    /// second one worked, so this is what it calls when it did not.
    pub fn release(&self, id: i64, now: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE task SET status = 'new', agent_session_id = NULL, owner = NULL, \
                 heartbeat_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![id, now],
            )
            .map_err(|problem| format!("ticket {id} could not be given back: {problem}"))?;
        Ok(())
    }

    /// Which window a ticket's worker belongs to, if any.
    ///
    /// Two Unluminous windows can have the same board file open, and each runs its own watchdog over the same
    /// rows. Without an owner, a window that has no terminal for a card reads it as a card whose worker is
    /// gone, and after the lease expires it strikes and reclaims work that is still running in the other
    /// window. So a claim records who took it, and a watchdog leaves alone a card owned by a window that is
    /// still running.
    pub fn owner_of(&self, id: i64) -> Result<Option<String>, String> {
        self.connection
            .query_row("SELECT owner FROM task WHERE id = ?1", params![id], |row| row.get(0))
            .optional()
            .map(Option::flatten)
            .map_err(|problem| format!("ticket {id}'s owner could not be read: {problem}"))
    }

    /// Record that a session exists for this ticket without changing which lane it is in.
    ///
    /// What `Resume session` writes. A ticket in Agent Done whose session is resumed stays in Agent
    /// Done, because resuming a conversation is not a claim on the work.
    pub fn set_session(&self, id: i64, session: &str, now: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE task SET agent_session_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, session, now],
            )
            .map_err(|problem| format!("the session could not be recorded: {problem}"))?;
        Ok(())
    }

    /// The agent proved it is working. Clears both watchdog counters, which is the rule that only
    /// board activity stops the nudges.
    pub fn heartbeat(&self, id: i64, minutes: Option<i64>, now: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE task SET heartbeat_at = ?3, lease_duration_minutes = COALESCE(?2, lease_duration_minutes), \
                 watchdog_strikes = 0, watchdog_nudges = 0, watchdog_nudged_at = NULL, updated_at = ?3 \
                 WHERE id = ?1",
                params![id, minutes, now],
            )
            .map_err(|problem| format!("the heartbeat could not be recorded: {problem}"))?;
        Ok(())
    }

    pub fn delete_task(&self, id: i64) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM task WHERE id = ?1", params![id])
            .map_err(|problem| format!("ticket {id} could not be deleted: {problem}"))?;
        Ok(())
    }

    pub fn add_todo(&self, task: i64, text: &str, now: &str) -> Result<Todo, String> {
        self.in_transaction(|| self.add_todo_now(task, text, now))
    }

    fn add_todo_now(&self, task: i64, text: &str, now: &str) -> Result<Todo, String> {
        let position: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM task_todo WHERE task_id = ?1",
                params![task],
                |row| row.get(0),
            )
            .map_err(|problem| format!("the todo's place could not be worked out: {problem}"))?;
        self.connection
            .execute(
                "INSERT INTO task_todo (task_id, text, done, position, created_at) \
                 VALUES (?1, ?2, 0, ?3, ?4)",
                params![task, text, position, now],
            )
            .map_err(|problem| format!("the todo could not be added: {problem}"))?;
        // Adding a todo is board activity, so it clears the watchdog's counters exactly as a comment
        // and a heartbeat do. Otherwise an agent that is plainly working would still be nudged.
        self.touch(task, now)?;
        let id = self.connection.last_insert_rowid();
        Ok(Todo {
            id,
            task_id: task,
            text: text.to_owned(),
            done: false,
            position,
            created_at: now.to_owned(),
        })
    }

    pub fn set_todo_done(&self, id: i64, done: bool, now: &str) -> Result<(), String> {
        self.in_transaction(|| self.set_todo_done_now(id, done, now))
    }

    fn set_todo_done_now(&self, id: i64, done: bool, now: &str) -> Result<(), String> {
        let task: Option<i64> = self
            .connection
            .query_row("SELECT task_id FROM task_todo WHERE id = ?1", params![id], |row| row.get(0))
            .optional()
            .map_err(|problem| format!("todo {id} could not be read: {problem}"))?;
        let task = task.ok_or_else(|| format!("todo {id} is not on the board"))?;
        self.connection
            .execute("UPDATE task_todo SET done = ?2 WHERE id = ?1", params![id, i64::from(done)])
            .map_err(|problem| format!("todo {id} could not be changed: {problem}"))?;
        self.touch(task, now)
    }

    pub fn set_todo_text(&self, id: i64, text: &str) -> Result<(), String> {
        self.connection
            .execute("UPDATE task_todo SET text = ?2 WHERE id = ?1", params![id, text])
            .map_err(|problem| format!("todo {id} could not be changed: {problem}"))?;
        Ok(())
    }

    pub fn delete_todo(&self, id: i64) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM task_todo WHERE id = ?1", params![id])
            .map_err(|problem| format!("todo {id} could not be deleted: {problem}"))?;
        Ok(())
    }

    pub fn add_comment(
        &self,
        task: i64,
        author: Author,
        body: &str,
        now: &str,
    ) -> Result<Comment, String> {
        self.in_transaction(|| self.add_comment_now(task, author, body, now))
    }

    fn add_comment_now(
        &self,
        task: i64,
        author: Author,
        body: &str,
        now: &str,
    ) -> Result<Comment, String> {
        self.connection
            .execute(
                "INSERT INTO task_comment (task_id, author, body, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![task, author.name(), body, now],
            )
            .map_err(|problem| format!("the comment could not be added: {problem}"))?;
        // **The board's own comments are not board activity.** `task-28` found this by driving the watchdog: a
        // strike posts a `system` comment, `touch` clears `watchdog_strikes`, so the strike erased itself and the
        // count could never reach `strikes_before_reclaim`. A ticket whose worker was gone was struck for ever and
        // never given back, which is the one thing the watchdog exists to do.
        //
        // A comment from a person or an agent is somebody saying something, and that is activity. A comment from
        // the watchdog is the watchdog talking to itself.
        if author != Author::System {
            self.touch(task, now)?;
        }
        Ok(Comment {
            id: self.connection.last_insert_rowid(),
            task_id: task,
            author,
            body: body.to_owned(),
            created_at: now.to_owned(),
        })
    }

    /// Change what a comment says, which is what `Edit` on a comment does.
    ///
    /// **Only a person's own comment.** The browser board offers `Edit` on a human's comments and not on an
    /// agent's, and the reason is not politeness: a comment written by an agent is a record of what the agent
    /// said, and rewriting it would make the ticket's history a thing nobody can rely on. The refusal is here,
    /// in the store, rather than only in the button, so the command line cannot get round it either.
    pub fn edit_comment(&self, id: i64, body: &str, now: &str) -> Result<Comment, String> {
        self.in_transaction(|| {
            let (task, author): (i64, String) = self
                .connection
                .query_row("SELECT task_id, author FROM task_comment WHERE id = ?1", params![id], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .map_err(|_| format!("there is no comment {id} on this board"))?;
            let author = Author::parse(&author).ok_or_else(|| {
                format!("comment {id} names `{author}`, which is not an author this board knows")
            })?;
            if author != Author::Human {
                return Err(format!(
                    "comment {id} was written by {}, and what an agent said is a record rather than a draft",
                    author.name()
                ));
            }
            if body.trim().is_empty() {
                return Err("a comment cannot be emptied; delete the ticket or leave it as it was".to_owned());
            }
            self.connection
                .execute("UPDATE task_comment SET body = ?2 WHERE id = ?1", params![id, body])
                .map_err(|problem| format!("comment {id} could not be changed: {problem}"))?;
            self.touch(task, now)?;
            Ok(Comment {
                id,
                task_id: task,
                author,
                body: body.to_owned(),
                // The time it was written, which an edit does not change: `created_at` is when it was said.
                created_at: self
                    .connection
                    .query_row("SELECT created_at FROM task_comment WHERE id = ?1", params![id], |row| {
                        row.get(0)
                    })
                    .unwrap_or_else(|_| now.to_owned()),
            })
        })
    }

    /// Board activity: the ticket changed, so the watchdog's counters go back to nothing.
    ///
    /// One function rather than the same three columns written in four places, because the fifth place
    /// added later is the one that would forget and leave a working agent being nudged.
    fn touch(&self, task: i64, now: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE task SET heartbeat_at = ?2, watchdog_strikes = 0, watchdog_nudges = 0, \
                 watchdog_nudged_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![task, now],
            )
            .map_err(|problem| format!("ticket {task} could not be touched: {problem}"))?;
        Ok(())
    }

    pub fn create_epic(&self, name: &str, color: &str) -> Result<Epic, String> {
        let position: i64 = self
            .connection
            .query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM task_epic", [], |row| row.get(0))
            .map_err(|problem| format!("the epic's place could not be worked out: {problem}"))?;
        self.connection
            .execute(
                "INSERT INTO task_epic (name, color, position) VALUES (?1, ?2, ?3)",
                params![name, color, position],
            )
            .map_err(|problem| format!("the epic could not be created: {problem}"))?;
        Ok(Epic {
            id: self.connection.last_insert_rowid(),
            name: name.to_owned(),
            color: color.to_owned(),
            position,
        })
    }

    /// Create a sprint. Making one active stands down whichever was active before, because the board
    /// shows one sprint and two active sprints would be two boards.
    pub fn create_sprint(&self, name: &str, status: SprintStatus, now: &str) -> Result<Sprint, String> {
        self.in_transaction(|| self.create_sprint_now(name, status, now))
    }

    fn create_sprint_now(
        &self,
        name: &str,
        status: SprintStatus,
        now: &str,
    ) -> Result<Sprint, String> {
        let position: i64 = self
            .connection
            .query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM sprint", [], |row| row.get(0))
            .map_err(|problem| format!("the sprint's place could not be worked out: {problem}"))?;
        if status == SprintStatus::Active {
            self.connection
                .execute("UPDATE sprint SET status = 'planned' WHERE status = 'active'", [])
                .map_err(|problem| format!("the previous sprint could not stand down: {problem}"))?;
        }
        self.connection
            .execute(
                "INSERT INTO sprint (name, status, position, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![name, status.name(), position, now],
            )
            .map_err(|problem| format!("the sprint could not be created: {problem}"))?;
        Ok(Sprint {
            id: self.connection.last_insert_rowid(),
            name: name.to_owned(),
            status,
            position,
            created_at: now.to_owned(),
        })
    }

    /// Every ticket in one sprint, in the order the board draws them.
    ///
    /// The Backlog view groups by sprint, which is what the page this board is modelled on does, so it asks
    /// for one group at a time rather than reading every ticket and sorting them here.
    pub fn tasks_of_sprint(&self, sprint: i64) -> Result<Vec<Task>, String> {
        let sql = format!("{TASK_COLUMNS} WHERE t.sprint_id = ?1 ORDER BY t.position, t.id");
        self.tasks_by(&sql, params![sprint])
    }

    /// Move one ticket into a sprint, or out of every sprint and into the backlog.
    ///
    /// **It goes to the foot of the lane it is already in**, rather than keeping a position that belonged to
    /// a different list: two sprints each number their tickets from zero, so a ticket carrying position 3
    /// into a sprint that has one would be drawn in the middle of nothing. That is the rule `move_task`
    /// already keeps for a ticket dragged between lanes.
    pub fn set_sprint_of(&self, id: i64, sprint: Option<i64>, now: &str) -> Result<(), String> {
        self.in_transaction(|| {
            let status: String = self
                .connection
                .query_row("SELECT status FROM task WHERE id = ?1", params![id], |row| row.get(0))
                .map_err(|problem| format!("the ticket could not be read: {problem}"))?;
            let position: i64 = self
                .connection
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM task                      WHERE status = ?1 AND sprint_id IS ?2",
                    params![status, sprint],
                    |row| row.get(0),
                )
                .map_err(|problem| format!("the ticket's place could not be worked out: {problem}"))?;
            self.connection
                .execute(
                    "UPDATE task SET sprint_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
                    params![id, sprint, position, now],
                )
                .map_err(|problem| format!("the ticket could not be moved: {problem}"))?;
            Ok(())
        })
    }

    /// Make one sprint the active one, standing the previous one down.
    ///
    /// One sprint is active at a time — the board draws that one — so this is two statements in one
    /// transaction rather than a caller remembering to run both.
    pub fn make_sprint_active(&self, id: i64) -> Result<(), String> {
        self.in_transaction(|| {
            self.connection
                .execute("UPDATE sprint SET status = 'planned' WHERE status = 'active'", [])
                .map_err(|problem| format!("the previous sprint could not stand down: {problem}"))?;
            self.connection
                .execute("UPDATE sprint SET status = 'active' WHERE id = ?1", params![id])
                .map_err(|problem| format!("the sprint could not be made active: {problem}"))?;
            Ok(())
        })
    }

    /// Close a sprint, and put whatever was not finished back in the backlog.
    ///
    /// **Unfinished means anything not in Agent Done**, which is the rule the browser board states in the
    /// question it asks before doing it. A completed sprint is a record of what was finished in it, so a
    /// ticket left in New would otherwise be filed under a fortnight it was not worked in and would never be
    /// seen again — the Completed view is read-only.
    pub fn complete_sprint(&self, id: i64, now: &str) -> Result<usize, String> {
        self.in_transaction(|| {
            let unfinished: Vec<i64> = {
                let mut statement = self.prepared(
                    "SELECT id FROM task WHERE sprint_id = ?1 AND status <> 'agent_done'",
                )?;
                let rows = statement
                    .query_map(params![id], |row| row.get(0))
                    .map_err(|problem| format!("the sprint's tickets could not be read: {problem}"))?;
                rows.collect::<Result<Vec<i64>, _>>()
                    .map_err(|problem| format!("a ticket could not be read: {problem}"))?
            };
            for task in &unfinished {
                self.to_the_foot_of_the_backlog(*task, now)?;
            }
            self.connection
                .execute("UPDATE sprint SET status = 'completed' WHERE id = ?1", params![id])
                .map_err(|problem| format!("the sprint could not be completed: {problem}"))?;
            Ok(unfinished.len())
        })
    }

    /// Rename a sprint.
    pub fn rename_sprint(&self, id: i64, name: &str) -> Result<(), String> {
        self.connection
            .execute("UPDATE sprint SET name = ?2 WHERE id = ?1", params![id, name])
            .map(|_| ())
            .map_err(|problem| format!("the sprint could not be renamed: {problem}"))
    }

    /// Take a sprint away. Its tickets go back to the backlog rather than with it.
    ///
    /// **Nothing on this board deletes a ticket as a side effect of deleting something else.** A sprint is
    /// a fortnight somebody named; the work in it is the work, and it outlives the name.
    pub fn delete_sprint(&self, id: i64, now: &str) -> Result<(), String> {
        self.in_transaction(|| {
            // One at a time, because each has to be given a place at the foot of the backlog's own lane —
            // see `to_the_foot_of_the_backlog`. A sprint holds tens of tickets, not thousands.
            let leaving: Vec<i64> = {
                let mut statement = self.prepared("SELECT id FROM task WHERE sprint_id = ?1")?;
                let rows = statement
                    .query_map(params![id], |row| row.get(0))
                    .map_err(|problem| format!("the sprint's tickets could not be read: {problem}"))?;
                rows.collect::<Result<Vec<i64>, _>>()
                    .map_err(|problem| format!("a ticket could not be read: {problem}"))?
            };
            for task in leaving {
                self.to_the_foot_of_the_backlog(task, now)?;
            }
            self.connection
                .execute("DELETE FROM sprint WHERE id = ?1", params![id])
                .map_err(|problem| format!("the sprint could not be deleted: {problem}"))?;
            Ok(())
        })
    }

    /// Put one ticket at the foot of the backlog's own lane, keeping the lane it is in.
    ///
    /// **A position belongs to a `(status, sprint)` list**, which is what `set_sprint_of` says and what
    /// completing or deleting a sprint used not to honour: a ticket carrying position 0 out of a sprint
    /// landed on top of the backlog's own position 0, so returned work was interleaved with what was
    /// already there instead of arriving after it. Found by the `task-1771` review.
    fn to_the_foot_of_the_backlog(&self, id: i64, now: &str) -> Result<(), String> {
        let status: String = self
            .connection
            .query_row("SELECT status FROM task WHERE id = ?1", params![id], |row| row.get(0))
            .map_err(|problem| format!("the ticket could not be read: {problem}"))?;
        let position: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM task \
                 WHERE status = ?1 AND sprint_id IS NULL",
                params![status],
                |row| row.get(0),
            )
            .map_err(|problem| format!("the ticket's place could not be worked out: {problem}"))?;
        self.connection
            .execute(
                "UPDATE task SET sprint_id = NULL, position = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, position, now],
            )
            .map_err(|problem| format!("a ticket could not go back to the backlog: {problem}"))?;
        Ok(())
    }

    /// Rename an epic, recolour it, or both. A `None` leaves that half alone.
    pub fn edit_epic(&self, id: i64, name: Option<&str>, color: Option<&str>) -> Result<(), String> {
        if let Some(name) = name {
            self.connection
                .execute("UPDATE task_epic SET name = ?2 WHERE id = ?1", params![id, name])
                .map_err(|problem| format!("the epic could not be renamed: {problem}"))?;
        }
        if let Some(color) = color {
            self.connection
                .execute("UPDATE task_epic SET color = ?2 WHERE id = ?1", params![id, color])
                .map_err(|problem| format!("the epic could not be recoloured: {problem}"))?;
        }
        Ok(())
    }

    /// Take an epic away. Its tickets keep existing with no epic, which is what the browser board does.
    pub fn delete_epic(&self, id: i64, now: &str) -> Result<(), String> {
        self.in_transaction(|| {
            self.connection
                .execute(
                    "UPDATE task SET epic_id = NULL, updated_at = ?2 WHERE epic_id = ?1",
                    params![id, now],
                )
                .map_err(|problem| format!("its tickets could not be freed of it: {problem}"))?;
            self.connection
                .execute("DELETE FROM task_epic WHERE id = ?1", params![id])
                .map_err(|problem| format!("the epic could not be deleted: {problem}"))?;
            Ok(())
        })
    }

    /// How many tickets name each epic, by epic id.
    ///
    /// Counted in the database rather than by walking the board, because the Epics view shows a count for
    /// every epic and the board holds only the active sprint's tickets — an epic used entirely in the
    /// backlog would otherwise read as zero.
    pub fn epic_counts(&self) -> Result<Vec<(i64, i64)>, String> {
        let mut statement =
            self.prepared("SELECT epic_id, COUNT(*) FROM task WHERE epic_id IS NOT NULL GROUP BY epic_id")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|problem| format!("the epics could not be counted: {problem}"))?;
        rows.collect::<Result<Vec<(i64, i64)>, _>>()
            .map_err(|problem| format!("an epic count could not be read: {problem}"))
    }

    /// How many tickets are in each sprint, by sprint id.
    pub fn sprint_counts(&self) -> Result<Vec<(i64, i64)>, String> {
        let mut statement = self
            .prepared("SELECT sprint_id, COUNT(*) FROM task WHERE sprint_id IS NOT NULL GROUP BY sprint_id")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|problem| format!("the sprints could not be counted: {problem}"))?;
        rows.collect::<Result<Vec<(i64, i64)>, _>>()
            .map_err(|problem| format!("a sprint count could not be read: {problem}"))
    }

    /// The schedules, in the order they run.
    pub fn schedules(&self) -> Result<Vec<Schedule>, String> {
        let mut statement = self.prepared(
            "SELECT id, project, agent, command, cron_expression, enabled, last_run_at, next_run_at, last_status \
             FROM task_schedule ORDER BY next_run_at IS NULL, next_run_at, id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(Schedule {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    agent: row.get(2)?,
                    command: row.get(3)?,
                    cron: row.get(4)?,
                    enabled: row.get::<_, i64>(5)? != 0,
                    last_run_at: row.get(6)?,
                    next_run_at: row.get(7)?,
                    last_status: row.get(8)?,
                })
            })
            .map_err(|problem| format!("the schedules could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Schedule>, _>>()
            .map_err(|problem| format!("a schedule could not be read: {problem}"))
    }

    // ------------------------------------------------------------------ the watchdog's own read

    /// The cards the watchdog polices, with how stale each one's board record is.
    ///
    /// `agent_session_id IS NOT NULL` is what makes a card the watchdog's business, and the reason is
    /// the one the application being replaced wrote down: the column is set only when a terminal
    /// launches, so a card without one has no worker to nudge or reclaim. It is in progress because a
    /// person put it there.
    ///
    /// Every such card is returned rather than only the ones whose lease expired, because an agent
    /// sitting at its prompt inside its lease has still stopped, and the caller decides which of the
    /// two failures it is.
    pub fn watchdog_candidates(
        &self,
        now: &str,
        default_lease_minutes: i64,
    ) -> Result<Vec<Candidate>, String> {
        let mut statement = self.prepared(
            "SELECT id, task_key, agent_session_id, watchdog_strikes, watchdog_nudges, \
                    watchdog_nudged_at, \
                    CAST((julianday(?1) - julianday(COALESCE(heartbeat_at, updated_at))) * 1440 AS INTEGER), \
                    COALESCE(lease_duration_minutes, ?2), \
                    CAST((julianday(?1) - julianday(COALESCE(watchdog_nudged_at, '1970-01-01'))) * 1440 AS INTEGER) \
             FROM task WHERE status = 'in_progress' AND agent_session_id IS NOT NULL \
             ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![now, default_lease_minutes], |row| {
                Ok(Candidate {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    session_id: row.get(2)?,
                    strikes: row.get(3)?,
                    nudges: row.get(4)?,
                    nudged_at: row.get(5)?,
                    board_idle_minutes: row.get(6)?,
                    lease_minutes: row.get(7)?,
                    minutes_since_nudge: row.get(8)?,
                })
            })
            .map_err(|problem| format!("the watchdog's cards could not be read: {problem}"))?;
        rows.collect::<Result<Vec<Candidate>, _>>()
            .map_err(|problem| format!("a watchdog card could not be read: {problem}"))
    }

    /// Record a strike against a card whose terminal is gone.
    pub fn strike(&self, id: i64) -> Result<i64, String> {
        self.connection
            .execute(
                "UPDATE task SET watchdog_strikes = watchdog_strikes + 1 WHERE id = ?1",
                params![id],
            )
            .map_err(|problem| format!("the strike could not be recorded: {problem}"))?;
        self.connection
            .query_row("SELECT watchdog_strikes FROM task WHERE id = ?1", params![id], |row| row.get(0))
            .map_err(|problem| format!("the strikes could not be read: {problem}"))
    }

    /// Record that a continue instruction was typed into a live terminal.
    pub fn nudge(&self, id: i64, now: &str) -> Result<i64, String> {
        self.connection
            .execute(
                "UPDATE task SET watchdog_nudges = watchdog_nudges + 1, watchdog_nudged_at = ?2 \
                 WHERE id = ?1",
                params![id, now],
            )
            .map_err(|problem| format!("the nudge could not be recorded: {problem}"))?;
        self.connection
            .query_row("SELECT watchdog_nudges FROM task WHERE id = ?1", params![id], |row| row.get(0))
            .map_err(|problem| format!("the nudges could not be read: {problem}"))
    }

    /// Give a card back to New, keeping its todos and its comments.
    ///
    /// The session id is cleared, because the conversation it named belonged to a worker that is gone
    /// and the next worker starts its own. The todos and comments are what it left behind for that next
    /// worker to read, which is why they are untouched.
    pub fn reclaim(&self, id: i64, now: &str) -> Result<(), String> {
        self.in_transaction(|| self.reclaim_now(id, now))
    }

    fn reclaim_now(&self, id: i64, now: &str) -> Result<(), String> {
        let was = self.task(id)?.ok_or_else(|| format!("ticket {id} is not on the board"))?;
        self.connection
            .execute(
                "UPDATE task SET status = 'new', agent_session_id = NULL, owner = NULL, \
                 heartbeat_at = NULL, watchdog_strikes = 0, watchdog_nudges = 0, \
                 watchdog_nudged_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![id, now],
            )
            .map_err(|problem| format!("ticket {id} could not be reclaimed: {problem}"))?;
        // A reclaim is a move like any other: the card goes to the foot of New **in its own sprint**, and
        // the lane it left has a hole where it was. Both are renumbered, and both are read with the card's
        // own sprint rather than with `None`, which would have meant the tickets that have no sprint.
        let new_lane = self.lane_order(Status::New, was.sprint_id)?;
        self.write_positions(&new_lane)?;
        if was.status != Status::New {
            let left_behind = self.lane_order(was.status, was.sprint_id)?;
            self.write_positions(&left_behind)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ searching

    /// Tickets whose key, title or description holds `query`, newest first.
    ///
    /// `LIKE` with the query between two wildcards rather than a full text index: the board is thousands
    /// of rows and a full text table would be a second copy of every description to keep in step, which
    /// is the one cost a search never pays today — nothing to invalidate when a ticket is edited.
    /// Measured by `examples/board_cost.rs`, this is 31 ms at worst on 5000 tickets. It runs when a key is
    /// pressed in the search box rather than once a frame, so it is paid where a person is already waiting
    /// for the answer; at ten times the tickets it would need the index.
    pub fn search(&self, query: &str) -> Result<Vec<Task>, String> {
        let sql = format!(
            "{TASK_COLUMNS} WHERE t.task_key LIKE ?1 OR LOWER(t.title) LIKE ?1 \
             OR LOWER(t.description) LIKE ?1 ORDER BY t.updated_at DESC, t.id DESC LIMIT 100"
        );
        let pattern = format!("%{}%", query.to_lowercase());
        self.tasks_by(&sql, params![pattern])
    }

    /// Run `work` as one transaction: all of its statements, or none of them.
    ///
    /// SQLite autocommits every statement on its own, so an operation made of several — creating a ticket
    /// after reading the next key, renumbering a lane after moving a card, standing a sprint down before
    /// making another active — can interleave with another window's and can stop half done. `BEGIN
    /// IMMEDIATE` takes the write lock at the start rather than on the first write, which is what stops two
    /// windows reading the same next key and both using it.
    ///
    /// `unchecked_transaction` rather than `transaction`, because the latter wants `&mut Connection` and
    /// every read here takes `&self`. The rollback still happens on drop, which is the part that matters.
    fn in_transaction<T>(
        &self,
        work: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|problem| format!("the board could not begin a transaction: {problem}"))?;
        let answer = work()?;
        transaction
            .commit()
            .map_err(|problem| format!("the board could not be written: {problem}"))?;
        Ok(answer)
    }

    fn prepared(&self, sql: &str) -> Result<rusqlite::Statement<'_>, String> {
        self.connection
            .prepare(sql)
            .map_err(|problem| format!("a query could not be prepared: {problem}\n{sql}"))
    }
}

/// One row of `task_schedule`: a command an agent runs on a clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    pub id: String,
    pub project: String,
    pub agent: String,
    pub command: String,
    /// The five field cron expression, as text. Read by whatever runs it rather than by the board.
    pub cron: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_status: Option<String>,
}

/// A card the watchdog is looking at, with the arithmetic already done by the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: i64,
    pub key: String,
    pub session_id: String,
    pub strikes: i64,
    pub nudges: i64,
    pub nudged_at: Option<String>,
    /// How long since the board last heard from the agent, by a todo, a comment or a heartbeat.
    pub board_idle_minutes: i64,
    /// This card's lease, or the default when it has not asked for one.
    pub lease_minutes: i64,
    /// How long since the last continue instruction was typed, and a large number when there has been
    /// none, so a first nudge is never held back by an interval it has not had yet.
    pub minutes_since_nudge: i64,
}

impl Candidate {
    pub fn lease_expired(&self) -> bool {
        self.board_idle_minutes >= self.lease_minutes
    }
}

/// A ticket about to be created.
#[derive(Debug, Clone, PartialEq)]
pub struct NewTask {
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub status: Status,
    pub assignee: Assignee,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub epic_id: Option<i64>,
    pub sprint_id: Option<i64>,
    pub project: Option<String>,
}

impl Default for NewTask {
    /// What `Add Task` creates: an empty ticket in New, for Claude, in the default project.
    ///
    /// The row exists from the moment the editor opens, which is what makes the editor able to save as
    /// it is typed into rather than having a Create button somebody forgets to press.
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            priority: Priority::Medium,
            status: Status::New,
            assignee: Assignee::Claude,
            model: None,
            effort: None,
            epic_id: None,
            sprint_id: None,
            project: None,
        }
    }
}

/// The fields an edit changes. `None` leaves one alone, so one field is one call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskEdit {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub assignee: Option<Assignee>,
    pub model: Option<Option<String>>,
    pub effort: Option<Option<String>>,
    pub epic_id: Option<Option<i64>>,
    pub project: Option<Option<String>>,
    /// The JIRA issue this ticket is about, which the JIRA panel on the ticket sets.
    ///
    /// Nothing here talks to JIRA — the board has no HTTP client — so this is the key somebody typed rather than
    /// a key a sync brought in. It is what makes the panel able to name an issue at all on a board whose tickets
    /// are all its own.
    pub jira_key: Option<Option<String>>,
}

/// The columns every ticket read selects, with the three counts the card shows.
///
/// The counts are counted rather than stored, so they cannot go stale: a todo deleted by a cascade when
/// its ticket went would have left a stored count naming rows that are not there.
const TASK_COLUMNS: &str = "SELECT t.id, t.task_key, t.title, t.description, t.priority, t.status, \
     t.assignee, t.model, t.effort, t.epic_id, t.sprint_id, t.position, t.project, \
     t.agent_session_id, t.heartbeat_at, t.lease_duration_minutes, t.watchdog_strikes, \
     t.watchdog_nudges, t.watchdog_nudged_at, t.source, t.jira_key, t.jira_url, t.jira_status, \
     t.jira_issue_type, t.created_at, t.updated_at, \
     (SELECT COUNT(*) FROM task_todo d WHERE d.task_id = t.id), \
     (SELECT COUNT(*) FROM task_todo d WHERE d.task_id = t.id AND d.done = 1), \
     (SELECT COUNT(*) FROM task_comment c WHERE c.task_id = t.id) \
     FROM task t";

/// Refuse a word the board does not know, naming the column it came from.
///
/// **A row nobody can explain is worse than an error**, which is `model.rs`' own rule, and defaulting one
/// here would have broken it: a status that quietly became `new` would draw in the wrong lane, and somebody
/// would move it back and watch it move again. The check constraints in the schema are what stop this
/// happening at all; this is what happens if a file is edited by hand or written by something else.
fn known<T>(column: &str, word: &str, parsed: Option<T>) -> rusqlite::Result<T> {
    parsed.ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("`{word}` is not a value this board's {column} can hold").into(),
        )
    })
}

fn read_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let priority: String = row.get(4)?;
    let status: String = row.get(5)?;
    let assignee: String = row.get(6)?;
    let source: String = row.get(19)?;
    Ok(Task {
        id: row.get(0)?,
        key: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        priority: known("priority", &priority, Priority::parse(&priority))?,
        status: known("status", &status, Status::parse(&status))?,
        assignee: known("assignee", &assignee, Assignee::parse(&assignee))?,
        model: row.get(7)?,
        effort: row.get(8)?,
        epic_id: row.get(9)?,
        sprint_id: row.get(10)?,
        position: row.get(11)?,
        project: row.get(12)?,
        session_id: row.get(13)?,
        heartbeat_at: row.get(14)?,
        lease_minutes: row.get(15)?,
        watchdog_strikes: row.get(16)?,
        watchdog_nudges: row.get(17)?,
        watchdog_nudged_at: row.get(18)?,
        source: known("source", &source, Source::parse(&source))?,
        jira_key: row.get(20)?,
        jira_url: row.get(21)?,
        jira_status: row.get(22)?,
        jira_issue_type: row.get(23)?,
        created_at: row.get(24)?,
        updated_at: row.get(25)?,
        todo_count: row.get(26)?,
        todo_done_count: row.get(27)?,
        comment_count: row.get(28)?,
    })
}

fn read_sprint(row: &rusqlite::Row<'_>) -> rusqlite::Result<Sprint> {
    let status: String = row.get(2)?;
    Ok(Sprint {
        id: row.get(0)?,
        name: row.get(1)?,
        status: known("sprint status", &status, SprintStatus::parse(&status))?,
        position: row.get(3)?,
        created_at: row.get(4)?,
    })
}

/// Add any column a later version of the schema needs.
///
/// Guarded by a read of `pragma_table_info`, because SQLite has no `ADD COLUMN IF NOT EXISTS` and
/// running the statement twice is an error rather than a no operation. Nothing is ever dropped and no
/// table is recreated, which is the rule the schema being replaced already kept.
fn migrate(connection: &Connection) -> Result<(), String> {
    // Every version adds its rows here rather than only changing `SCHEMA`, so a board somebody has been
    // using is expanded in place. `SCHEMA` creates its tables with `CREATE TABLE IF NOT EXISTS`, so a
    // column added there alone never reaches a file that already exists — which is what left every board
    // written before `owner` refusing `Store::claim` with `no such column: owner`, and therefore refusing
    // to launch an agent at all.
    const ADDITIONS: &[(&str, &str, &str)] = &[
        // Schema 2. Which window holds a card, as `pid:<number>`. See `agent_tasks::owner_is_gone`.
        ("task", "owner", "TEXT"),
    ];
    for (table, column, definition) in ADDITIONS {
        if !has_column(connection, table, column)? {
            connection
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"), [])
                .map_err(|problem| format!("{table}.{column} could not be added: {problem}"))?;
        }
    }
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .map_err(|problem| format!("{table} could not be inspected: {problem}"))?;
    let names: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .map_err(|problem| format!("{table} could not be inspected: {problem}"))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|problem| format!("{table} could not be inspected: {problem}"))?;
    Ok(names.iter().any(|name| name == column))
}

/// The tables, with the four check constraints that are the board's rules written into the file.
///
/// A card in a lane that does not exist, a priority nothing draws, an assignee nothing can launch and a
/// sprint in a state nothing reads are all refused by the database rather than by the code that happens
/// to be writing that day.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS meta (
  name  TEXT PRIMARY KEY,
  value INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS task_epic (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  name     TEXT NOT NULL UNIQUE,
  color    TEXT NOT NULL DEFAULT '#2F6BFF',
  position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sprint (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  status     TEXT NOT NULL DEFAULT 'planned',
  position   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  CONSTRAINT sprint_status_check CHECK (status IN ('planned','active','completed'))
);
CREATE TABLE IF NOT EXISTS task (
  id                     INTEGER PRIMARY KEY AUTOINCREMENT,
  task_key               TEXT UNIQUE,
  title                  TEXT NOT NULL DEFAULT '',
  description            TEXT NOT NULL DEFAULT '',
  priority               TEXT NOT NULL DEFAULT 'medium',
  status                 TEXT NOT NULL DEFAULT 'new',
  assignee               TEXT NOT NULL DEFAULT 'claude',
  model                  TEXT,
  effort                 TEXT,
  epic_id                INTEGER REFERENCES task_epic(id) ON DELETE SET NULL,
  sprint_id              INTEGER REFERENCES sprint(id) ON DELETE SET NULL,
  position               INTEGER NOT NULL DEFAULT 0,
  project                TEXT,
  agent_session_id       TEXT,
  owner                  TEXT,
  heartbeat_at           TEXT,
  lease_duration_minutes INTEGER,
  watchdog_strikes       INTEGER NOT NULL DEFAULT 0,
  watchdog_nudges        INTEGER NOT NULL DEFAULT 0,
  watchdog_nudged_at     TEXT,
  source                 TEXT NOT NULL DEFAULT 'local',
  jira_key               TEXT,
  jira_url               TEXT,
  jira_status            TEXT,
  jira_issue_type        TEXT,
  created_at             TEXT NOT NULL,
  updated_at             TEXT NOT NULL,
  CONSTRAINT task_priority_check CHECK (priority IN ('low','medium','high')),
  CONSTRAINT task_status_check   CHECK (status IN ('new','in_progress','agent_done','qa_failed')),
  CONSTRAINT task_assignee_check CHECK (assignee IN ('claude','codex','human'))
);
CREATE INDEX IF NOT EXISTS task_status_idx ON task(status);
CREATE INDEX IF NOT EXISTS task_sprint_idx ON task(sprint_id);
CREATE TABLE IF NOT EXISTS task_todo (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id    INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  text       TEXT NOT NULL,
  done       INTEGER NOT NULL DEFAULT 0,
  position   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS task_todo_task_idx ON task_todo(task_id);
CREATE TABLE IF NOT EXISTS task_comment (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id    INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  author     TEXT NOT NULL DEFAULT 'claude',
  body       TEXT NOT NULL,
  created_at TEXT NOT NULL,
  CONSTRAINT task_comment_author_check CHECK (author IN ('human','claude','codex','system'))
);
CREATE INDEX IF NOT EXISTS task_comment_task_idx ON task_comment(task_id);
CREATE TABLE IF NOT EXISTS task_schedule (
  id              TEXT PRIMARY KEY,
  project         TEXT NOT NULL,
  agent           TEXT NOT NULL DEFAULT 'claude',
  command         TEXT NOT NULL,
  cron_expression TEXT NOT NULL,
  enabled         INTEGER NOT NULL DEFAULT 1,
  last_run_at     TEXT,
  next_run_at     TEXT,
  last_status     TEXT,
  created_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS task_schedule_due_idx ON task_schedule(enabled, next_run_at);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_tasks::model::Author;

    const NOW: &str = "2026-08-29T12:00:00Z";
    const LATER: &str = "2026-08-29T13:30:00Z";

    fn board() -> Store {
        Store::in_memory().expect("a board in memory")
    }

    /// A board with an active sprint and three tickets in it, which is what most of these read.
    fn filled() -> (Store, i64) {
        let store = board();
        let sprint = store.create_sprint("Current Sprint", SprintStatus::Active, NOW).expect("a sprint");
        for title in ["First", "Second", "Third"] {
            store
                .create_task(
                    NewTask { title: title.to_owned(), sprint_id: Some(sprint.id), ..NewTask::default() },
                    NOW,
                )
                .expect("a ticket");
        }
        (store, sprint.id)
    }

    #[test]
    fn opening_a_file_that_is_not_there_makes_the_schema_and_opening_it_again_changes_nothing() {
        let folder = std::env::temp_dir().join(format!("unluminous-board-{}", std::process::id()));
        let path = folder.join("board.sqlite3");
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).expect("a new board");
            store.create_task(NewTask { title: "A".to_owned(), ..NewTask::default() }, NOW).expect("a ticket");
        }
        // Opening it again reads the ticket back rather than recreating anything, which is what
        // `CREATE TABLE IF NOT EXISTS` and an additive migration are for.
        let again = Store::open(&path).expect("the same board");
        assert_eq!(again.board().expect("the board").total(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// `task-28`: the operator's board answered `ticket 7 could not be claimed: no such column: owner`, and no
    /// agent could be launched on it at all.
    ///
    /// The cause, reproduced here rather than described: `SCHEMA` creates `task` with `CREATE TABLE IF NOT
    /// EXISTS`, so a file that already exists keeps the columns it was made with, and `owner` was added to
    /// `SCHEMA` after this machine's board file was made. `migrate` is what adds a column to a file that has
    /// been used, and its list was empty.
    #[test]
    fn a_board_made_before_owner_existed_gains_it_and_a_ticket_can_then_be_claimed() {
        let folder = std::env::temp_dir().join(format!("unluminous-board-owner-{}", std::process::id()));
        let path = folder.join(FILE);
        let _ = std::fs::remove_dir_all(&folder);
        let id = {
            let store = Store::open(&path).expect("a new board");
            let task = store
                .create_task(NewTask { title: "Claim me".to_owned(), ..NewTask::default() }, NOW)
                .expect("a ticket");
            // The state the operator's file is in: `owner` is not on the table and the version says 1.
            store
                .connection
                .execute_batch(
                    "ALTER TABLE task DROP COLUMN owner;\n\
                     UPDATE meta SET value = 1 WHERE name = 'schema_version';",
                )
                .expect("a board from before the column existed");
            assert!(
                !has_column(&store.connection, "task", "owner").expect("a read"),
                "the column has to be gone for this to be the reported failure"
            );
            assert!(
                store.claim(task.id, "a-session", Assignee::Claude, "pid:1", NOW).is_err(),
                "this is the failure the operator saw, and it has to happen before the fix is proved"
            );
            task.id
        };

        // Opening it again is what migrates it, which is what happens when Unluminous starts.
        let again = Store::open(&path).expect("the same board");
        assert!(has_column(&again.connection, "task", "owner").expect("a read"), "the column was added");
        let claimed = again
            .claim(id, "a-session", Assignee::Claude, "pid:1", NOW)
            .expect("the claim that used to fail");
        assert!(claimed, "the ticket was in New, so this caller got it");
        assert_eq!(again.owner_of(id).expect("the owner").as_deref(), Some("pid:1"));
        // And the file now says which schema it is at, so the next column added to it can be reasoned about.
        let version: i64 = again
            .connection
            .query_row("SELECT value FROM meta WHERE name = 'schema_version'", [], |row| row.get(0))
            .expect("the version");
        assert_eq!(version, SCHEMA_VERSION, "the version is written after the migration, not before it");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// `task-28`: "Clear out existing tasks. They were cloned and are out of date."
    ///
    /// The tickets go with their todos and their comments, and the epics and the sprint stay.
    #[test]
    fn clearing_the_board_takes_the_tickets_their_todos_and_their_comments_and_leaves_the_epics() {
        let (store, sprint) = filled();
        let epic = store.create_epic("An epic", "#2F6BFF").expect("an epic");
        let first = store.task_by_key("task-1").expect("a read").expect("the first ticket");
        store.add_todo(first.id, "Do the thing", NOW).expect("a todo");
        store.add_todo(first.id, "Do the other thing", NOW).expect("another todo");
        store.add_comment(first.id, Author::Human, "Started.", NOW).expect("a comment");
        assert_eq!(store.board().expect("the board").total(), 3);

        let (tickets, todos, comments) = store.clear_the_tickets().expect("a clear");
        assert_eq!((tickets, todos, comments), (3, 2, 1), "it says what it deleted");
        let board = store.board().expect("the board");
        assert_eq!(board.total(), 0, "no tickets");
        // The rows really are gone rather than orphaned, which is what the cascade is for. Asserted rather than
        // trusted, because the cascade only works while `PRAGMA foreign_keys` is on.
        let count = |table: &str| -> i64 {
            store
                .connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .expect("a count")
        };
        assert_eq!(count("task_todo"), 0, "the todos went with their tickets");
        assert_eq!(count("task_comment"), 0, "and so did the comments");
        // And what is not a ticket stays: the sprint the board draws against, and the epics.
        assert_eq!(board.sprint.as_ref().map(|it| it.id), Some(sprint), "the active sprint is still active");
        assert!(board.epics.iter().any(|it| it.id == epic.id), "the epics are not tickets");
    }

    /// The copy the clear takes first, which is what makes it recoverable.
    ///
    /// `VACUUM INTO` rather than a file copy, because the board is in write ahead logging mode and the newest rows
    /// may still be in the `-wal` file. This is the test that would fail if that were a `std::fs::copy`: the
    /// ticket is written and the copy is taken in the same breath, with nothing in between to checkpoint it.
    #[test]
    fn the_copy_taken_before_a_clear_holds_the_tickets_that_were_there() {
        let folder = std::env::temp_dir().join(format!("unluminous-board-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join(FILE);
        let store = Store::open(&path).expect("a board on disk");
        store.create_sprint("Current Sprint", SprintStatus::Active, NOW).expect("a sprint");
        for title in ["One", "Two"] {
            store
                .create_task(NewTask { title: title.to_owned(), ..NewTask::default() }, NOW)
                .expect("a ticket");
        }
        let copy = folder.join("board-before-clear.sqlite3");
        let made = store.copy_the_file(&copy).expect("a copy");
        assert_eq!(made, copy);
        store.clear_the_tickets().expect("a clear");
        assert_eq!(store.board().expect("the board").total(), 0, "the board is empty");

        // The copy still holds both, including the one written last, which is the point of `VACUUM INTO`.
        let before = Store::open(&copy).expect("the copy opens as a board");
        let titles: Vec<String> = before
            .connection
            .prepare("SELECT title FROM task ORDER BY id")
            .and_then(|mut statement| {
                statement.query_map([], |row| row.get::<usize, String>(0))?.collect::<Result<Vec<String>, _>>()
            })
            .expect("the titles");
        assert_eq!(titles, vec!["One".to_owned(), "Two".to_owned()]);

        // A board in memory has no file, and says so rather than writing something misleading.
        let memory = board();
        assert!(memory.copy_the_file(&copy).is_err(), "there is no file to copy");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// `task-28`: the watchdog could never give work back, and the integration test that drives it is what found
    /// it.
    ///
    /// A strike records itself on the row and then posts a `system` comment saying so. Adding a comment used to
    /// count as board activity whoever wrote it, and board activity clears `watchdog_strikes` — so the strike
    /// erased itself, the count never reached `strikes_before_reclaim`, and a ticket whose worker had gone was
    /// warned for ever and never reclaimed.
    #[test]
    fn the_watchdogs_own_comment_does_not_clear_the_strike_it_just_recorded() {
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("the first ticket");
        store.claim(task.id, "a-session", Assignee::Claude, "pid:1", NOW).expect("a claim");
        assert_eq!(store.strike(task.id).expect("a strike"), 1);

        // What the watchdog does next, which used to undo what it had just done.
        store.add_comment(task.id, Author::System, "task-1 has not said anything.", LATER).expect("a comment");
        let candidate = store
            .watchdog_candidates(LATER, 45)
            .expect("the candidates")
            .into_iter()
            .find(|card| card.key == "task-1")
            .expect("the ticket is still a candidate");
        assert_eq!(candidate.strikes, 1, "the board's own comment is not somebody saying something");

        // A person's comment **is** activity, and clears it. That half is what `touch` is for and it stays.
        store.add_comment(task.id, Author::Human, "Still going, give it a minute.", LATER).expect("a comment");
        let candidate = store
            .watchdog_candidates(LATER, 45)
            .expect("the candidates")
            .into_iter()
            .find(|card| card.key == "task-1")
            .expect("the ticket");
        assert_eq!(candidate.strikes, 0, "somebody said something, so the count starts again");
    }

    #[test]
    fn a_board_written_by_a_newer_unluminous_is_refused_with_a_message() {
        // Reading it would mean reading columns this version does not know about, and the failure would
        // arrive later as a board with cards missing. Saying so when the file is opened is honest.
        let store = board();
        store
            .connection
            .execute("UPDATE meta SET value = ?1 WHERE name = 'schema_version'", params![SCHEMA_VERSION + 1])
            .expect("a newer version");
        let problem = store.check_version().expect_err("a newer schema should be refused");
        assert!(problem.contains("newer Unluminous"), "{problem}");
        assert!(problem.contains(&(SCHEMA_VERSION + 1).to_string()), "{problem}");
    }

    #[test]
    fn a_ticket_gets_the_next_key_and_the_tenth_is_not_given_a_key_that_exists() {
        let store = board();
        for expected in 1..=11 {
            let task = store.create_task(NewTask::default(), NOW).expect("a ticket");
            assert_eq!(task.key, format!("task-{expected}"));
        }
        // The reason the key is read as a number: `task-9` sorts after `task-10` as text, so reading the
        // highest key as a string would give the eleventh ticket a key the ninth already has.
        assert!(store.task_by_key("task-11").expect("a read").is_some());
    }

    #[test]
    fn a_word_the_board_does_not_know_is_refused_when_the_row_is_read() {
        // The check constraints below are what stop this happening at all. This is what happens when a board
        // file is edited by hand or written by something else: the read says which column and which word,
        // rather than drawing the card in whichever lane the default named.
        let store = board();
        let task = store.create_task(NewTask::default(), NOW).expect("a ticket");
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("the constraints off, so a bad row can be written at all");
        store
            .connection
            .execute("UPDATE task SET status = 'done' WHERE id = ?1", params![task.id])
            .expect("a row nothing can explain");
        let problem = store.task(task.id).expect_err("a status the board does not know");
        assert!(problem.contains("done"), "{problem}");
        assert!(problem.contains("status"), "it says which column: {problem}");
    }

    #[test]
    fn the_four_check_constraints_are_the_boards_rules_written_into_the_file() {
        let store = board();
        let task = store.create_task(NewTask::default(), NOW).expect("a ticket");
        for (column, value) in [
            ("status", "done"),
            ("priority", "urgent"),
            ("assignee", "gemini"),
        ] {
            let problem = store
                .connection
                .execute(&format!("UPDATE task SET {column} = ?2 WHERE id = ?1"), params![task.id, value])
                .expect_err(&format!("{column} = {value} should be refused by the database"));
            assert!(
                problem.to_string().to_lowercase().contains("constraint"),
                "{column}: {problem}"
            );
        }
    }

    #[test]
    fn a_persons_comment_can_be_changed_and_an_agents_cannot() {
        // `Edit` on a comment is offered on a person's own and on nobody else's, and this is where that is
        // decided: a comment an agent wrote is a record of what the agent said, and a board whose history can be
        // rewritten is a board nobody can use as evidence.
        let store = board();
        let ticket = store
            .create_task(NewTask { title: "A ticket".to_owned(), ..NewTask::default() }, NOW)
            .expect("the ticket");
        let now = LATER;
        let mine = store
            .add_comment(ticket.id, Author::Human, "The forma changed in April.", now)
            .expect("my comment");
        let changed = store
            .edit_comment(mine.id, "The format changed in April.", now)
            .expect("my own comment can be changed");
        assert_eq!(changed.body, "The format changed in April.");
        assert_eq!(changed.created_at, mine.created_at, "when it was said does not change");
        assert_eq!(store.comments(ticket.id).expect("the comments").len(), 1, "changed, not added to");

        let theirs = store
            .add_comment(ticket.id, Author::Claude, "I read the old importer.", now)
            .expect("the agent's comment");
        let problem = store
            .edit_comment(theirs.id, "I did not read it.", now)
            .expect_err("an agent's comment is refused");
        assert!(problem.contains("record rather than a draft"), "and it says why: `{problem}`");

        let missing = store.edit_comment(9999, "nothing", now).expect_err("no such comment");
        assert!(missing.contains("no comment 9999"), "and a comment that is not there says so: `{missing}`");

        let emptied = store.edit_comment(mine.id, "   ", now).expect_err("emptying is refused");
        assert!(emptied.contains("cannot be emptied"), "`{emptied}`");
    }

    #[test]
    fn deleting_a_ticket_deletes_its_todos_and_comments_and_nothing_else() {
        let (store, sprint) = filled();
        let kept = store.task_by_key("task-1").expect("a read").expect("task-1");
        let going = store.task_by_key("task-2").expect("a read").expect("task-2");
        store.add_todo(kept.id, "kept", NOW).expect("a todo");
        store.add_todo(going.id, "going", NOW).expect("a todo");
        store.add_comment(going.id, Author::Human, "going", NOW).expect("a comment");
        store.delete_task(going.id).expect("a delete");
        assert!(store.todos(going.id).expect("a read").is_empty(), "the cascade took its todos");
        assert!(store.comments(going.id).expect("a read").is_empty(), "and its comments");
        assert_eq!(store.todos(kept.id).expect("a read").len(), 1, "and nothing else");
        assert_eq!(store.board().expect("the board").total(), 2);
        let _ = sprint;
    }

    #[test]
    fn the_board_has_four_lanes_even_when_three_are_empty() {
        let (store, _) = filled();
        let read = store.board().expect("the board");
        assert_eq!(read.lanes.len(), 4);
        let names: Vec<&str> = read.lanes.iter().map(|lane| lane.status.name()).collect();
        assert_eq!(names, ["new", "qa_failed", "in_progress", "agent_done"], "in drawn order");
        assert_eq!(read.lane(Status::New).expect("New").count(), 3);
        assert_eq!(read.lane(Status::QaFailed).expect("QA Failed").count(), 0);
        assert_eq!(read.sprint.expect("the active sprint").name, "Current Sprint");
    }

    #[test]
    fn a_lanes_positions_stay_contiguous_when_a_card_leaves_it() {
        let (store, _) = filled();
        let second = store.task_by_key("task-2").expect("a read").expect("task-2");
        store.move_task(second.id, Status::InProgress, 0, LATER).expect("a move");
        let read = store.board().expect("the board");
        let new_lane = read.lane(Status::New).expect("New");
        assert_eq!(new_lane.count(), 2);
        let positions: Vec<i64> = new_lane.tasks.iter().map(|task| task.position).collect();
        assert_eq!(positions, [0, 1], "the hole the card left is closed");
        let moved = read.lane(Status::InProgress).expect("In Progress");
        assert_eq!(moved.count(), 1);
        assert_eq!(moved.tasks[0].position, 0);
    }

    #[test]
    fn a_card_moved_within_its_lane_lands_where_it_was_dropped() {
        let (store, _) = filled();
        let third = store.task_by_key("task-3").expect("a read").expect("task-3");
        store.move_task(third.id, Status::New, 0, LATER).expect("a move to the top");
        let keys: Vec<String> = store
            .board()
            .expect("the board")
            .lane(Status::New)
            .expect("New")
            .tasks
            .iter()
            .map(|task| task.key.clone())
            .collect();
        assert_eq!(keys, ["task-3", "task-1", "task-2"]);
    }

    #[test]
    fn a_ticket_is_claimed_once_and_every_later_caller_is_refused() {
        // Two windows pressing Start on one card would both see `new` and both launch an agent on it, and
        // two agents on one ticket is the worst thing this board can do. **Only an unclaimed ticket can be
        // claimed** — not even the same session id claims twice, because a second Start with the same id is
        // still a second agent. Handing a conversation back is `set_session`, which is a different thing and
        // says so.
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        assert!(store.claim(task.id, "session-one", Assignee::Claude, "pid:1", NOW).expect("a claim"));
        assert!(
            !store.claim(task.id, "session-two", Assignee::Codex, "pid:2", NOW).expect("a second claim"),
            "another window is refused"
        );
        assert!(
            !store.claim(task.id, "session-one", Assignee::Claude, "pid:1", LATER).expect("the same session"),
            "and so is the same session asking twice, which is still a second agent"
        );
        let claimed = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(claimed.status, Status::InProgress);
        assert_eq!(claimed.session_id.as_deref(), Some("session-one"));
        assert_eq!(claimed.assignee, Assignee::Claude);
        assert_eq!(store.owner_of(task.id).expect("the owner").as_deref(), Some("pid:1"));
        // Giving the claim back is what a failed spawn does, and then it can be claimed again.
        store.release(task.id, LATER).expect("a release");
        let given_back = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(given_back.status, Status::New);
        assert_eq!(given_back.session_id, None);
        assert_eq!(store.owner_of(task.id).expect("the owner"), None);
        assert!(store.claim(task.id, "session-two", Assignee::Codex, "pid:2", LATER).expect("a new claim"));
    }

    #[test]
    fn recording_a_session_does_not_change_which_lane_the_ticket_is_in() {
        // What `Resume session` writes. Resuming a conversation is not a claim on the work, so a ticket in
        // Agent Done that is resumed stays in Agent Done.
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        store.move_task(task.id, Status::AgentDone, 0, NOW).expect("a move");
        store.set_session(task.id, "an-old-session", LATER).expect("a session");
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(read.status, Status::AgentDone);
        assert_eq!(read.session_id.as_deref(), Some("an-old-session"));
    }

    #[test]
    fn a_todo_a_comment_and_a_heartbeat_all_count_as_board_activity() {
        // Only board activity stops the watchdog's nudges, so all three have to clear the counters. The
        // fifth place that changes a ticket would be the one that forgot, which is why they share
        // `touch`.
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        for (what, act) in [
            ("a todo", 0),
            ("a comment", 1),
            ("a heartbeat", 2),
        ] {
            store
                .connection
                .execute(
                    "UPDATE task SET watchdog_strikes = 2, watchdog_nudges = 2, \
                     watchdog_nudged_at = ?2, heartbeat_at = NULL WHERE id = ?1",
                    params![task.id, NOW],
                )
                .expect("some counters");
            match act {
                0 => {
                    store.add_todo(task.id, "something", LATER).expect(what);
                }
                1 => {
                    store.add_comment(task.id, Author::Claude, "something", LATER).expect(what);
                }
                _ => store.heartbeat(task.id, Some(90), LATER).expect(what),
            }
            let read = store.task(task.id).expect("a read").expect("task-1");
            assert_eq!(read.watchdog_strikes, 0, "{what} should clear the strikes");
            assert_eq!(read.watchdog_nudges, 0, "{what} should clear the nudges");
            assert_eq!(read.watchdog_nudged_at, None, "{what} should clear when it was nudged");
            assert_eq!(read.heartbeat_at.as_deref(), Some(LATER), "{what} is being heard from");
        }
        assert_eq!(
            store.task(task.id).expect("a read").expect("task-1").lease_minutes,
            Some(90),
            "a heartbeat that asked for a longer lease got one"
        );
    }

    #[test]
    fn the_counts_a_card_shows_are_counted_rather_than_stored() {
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        let first = store.add_todo(task.id, "one", NOW).expect("a todo");
        store.add_todo(task.id, "two", NOW).expect("a todo");
        store.add_comment(task.id, Author::Human, "hello", NOW).expect("a comment");
        store.set_todo_done(first.id, true, NOW).expect("a tick");
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!((read.todo_done_count, read.todo_count, read.comment_count), (1, 2, 1));
        store.delete_todo(first.id).expect("a delete");
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(
            (read.todo_done_count, read.todo_count),
            (0, 1),
            "a stored count would still be naming a row that is not there"
        );
    }

    #[test]
    fn the_watchdog_reads_only_cards_with_a_worker_and_does_the_arithmetic_in_the_database() {
        let (store, _) = filled();
        let watched = store.task_by_key("task-1").expect("a read").expect("task-1");
        let unwatched = store.task_by_key("task-2").expect("a read").expect("task-2");
        // In progress with a session: the watchdog's business.
        store.claim(watched.id, "a-session", Assignee::Claude, "pid:1", NOW).expect("a claim");
        // In progress with no session, which is a card a person put there. Striking it would comment on
        // and then reclaim work the watchdog was never policing.
        store.move_task(unwatched.id, Status::InProgress, 0, NOW).expect("a move");
        let candidates = store.watchdog_candidates(LATER, 45).expect("the candidates");
        assert_eq!(candidates.len(), 1, "only the card with a session");
        let card = &candidates[0];
        assert_eq!(card.key, "task-1");
        assert_eq!(card.session_id, "a-session");
        assert_eq!(card.board_idle_minutes, 90, "12:00 to 13:30 is ninety minutes");
        assert_eq!(card.lease_minutes, 45, "the default, because this card asked for none");
        assert!(card.lease_expired());
        assert!(
            card.minutes_since_nudge > 1000,
            "a card that has never been nudged is never held back by an interval it has not had"
        );
    }

    #[test]
    fn a_reclaim_keeps_the_todos_and_the_comments_and_clears_the_session() {
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        store.claim(task.id, "a-session", Assignee::Claude, "pid:1", NOW).expect("a claim");
        store.add_todo(task.id, "what was left to do", NOW).expect("a todo");
        store.add_comment(task.id, Author::Claude, "what I found", NOW).expect("a comment");
        store.reclaim(task.id, LATER).expect("a reclaim");
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(read.status, Status::New);
        assert_eq!(read.session_id, None, "the conversation belonged to the worker that is gone");
        assert_eq!(read.watchdog_strikes, 0);
        assert_eq!(read.todo_count, 1, "the next worker reads what the last one left");
        assert_eq!(read.comment_count, 1);
    }

    #[test]
    fn the_strikes_and_the_nudges_are_counted_separately() {
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        assert_eq!(store.strike(task.id).expect("a strike"), 1);
        assert_eq!(store.strike(task.id).expect("a strike"), 2);
        assert_eq!(store.nudge(task.id, NOW).expect("a nudge"), 1);
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(read.watchdog_strikes, 2);
        assert_eq!(read.watchdog_nudges, 1);
        assert_eq!(read.watchdog_nudged_at.as_deref(), Some(NOW));
    }

    #[test]
    fn the_search_reads_the_key_the_title_and_the_description() {
        let (store, sprint) = filled();
        let task = store
            .create_task(
                NewTask {
                    title: "Plugin architecture".to_owned(),
                    description: "The board is drawn in Rust.".to_owned(),
                    sprint_id: Some(sprint),
                    ..NewTask::default()
                },
                NOW,
            )
            .expect("a ticket");
        let keys = |found: Vec<Task>| -> Vec<String> { found.into_iter().map(|task| task.key).collect() };
        let only = [task.key.clone()];
        assert_eq!(keys(store.search("plugin").expect("a search")), only);
        assert_eq!(keys(store.search("PLUGIN").expect("a search")), only, "case insensitive");
        assert_eq!(keys(store.search("rust").expect("a search")), only, "the description too");
        assert_eq!(keys(store.search(&task.key).expect("a search")), only, "and the key");
        assert!(store.search("mermaid").expect("a search").is_empty());
    }

    #[test]
    fn a_ticket_with_no_sprint_is_the_backlog_and_a_closed_sprints_tickets_are_completed() {
        let (store, sprint) = filled();
        store
            .create_task(NewTask { title: "Someday".to_owned(), ..NewTask::default() }, NOW)
            .expect("a backlog ticket");
        let backlog = store.backlog().expect("the backlog");
        assert_eq!(backlog.len(), 1);
        assert_eq!(backlog[0].title, "Someday");
        assert!(store.completed().expect("completed").is_empty(), "no sprint is finished yet");
        store
            .connection
            .execute("UPDATE sprint SET status = 'completed' WHERE id = ?1", params![sprint])
            .expect("a finished sprint");
        assert_eq!(store.completed().expect("completed").len(), 3);
    }

    #[test]
    fn making_a_sprint_active_stands_the_previous_one_down() {
        // The board shows one sprint, so two active sprints would be two boards.
        let store = board();
        let first = store.create_sprint("One", SprintStatus::Active, NOW).expect("a sprint");
        let second = store.create_sprint("Two", SprintStatus::Active, NOW).expect("a sprint");
        assert_eq!(store.active_sprint().expect("the active sprint").expect("one").id, second.id);
        let sprints = store.sprints().expect("the sprints");
        let stood_down = sprints.iter().find(|sprint| sprint.id == first.id).expect("the first");
        assert_eq!(stood_down.status, SprintStatus::Planned);
    }

    #[test]
    fn a_board_with_no_sprint_shows_the_tickets_that_have_no_sprint_and_not_every_ticket() {
        // A board nobody has organised into sprints shows what is on it rather than four empty lanes. What it
        // must **not** do is show every ticket that ever existed: `?1 IS NULL OR sprint_id = ?1` means every
        // row when the argument is NULL, which is what this pins.
        let store = board();
        let old = store.create_sprint("Last month", SprintStatus::Completed, NOW).expect("a sprint");
        store
            .create_task(NewTask { title: "Finished".to_owned(), sprint_id: Some(old.id), ..NewTask::default() }, NOW)
            .expect("a ticket in a closed sprint");
        store.create_task(NewTask { title: "Loose".to_owned(), ..NewTask::default() }, NOW).expect("a ticket");
        let read = store.board().expect("the board");
        assert!(read.sprint.is_none(), "no sprint is active");
        assert_eq!(read.total(), 1, "the loose ticket, and not the one in the closed sprint");
        assert_eq!(read.lane(Status::New).expect("New").tasks[0].title, "Loose");
        // And with a sprint active, that sprint's tickets and nothing else.
        let current = store.create_sprint("This month", SprintStatus::Active, NOW).expect("a sprint");
        store
            .create_task(NewTask { title: "Current".to_owned(), sprint_id: Some(current.id), ..NewTask::default() }, NOW)
            .expect("a ticket");
        let read = store.board().expect("the board");
        assert_eq!(read.total(), 1);
        assert_eq!(read.lane(Status::New).expect("New").tasks[0].title, "Current");
    }

    #[test]
    fn a_reclaim_puts_the_card_at_the_foot_of_new_in_its_own_sprint_and_closes_the_gap_it_left() {
        let (store, sprint) = filled();
        // A ticket in another sprint, which a reclaim must not renumber.
        let other = store.create_sprint("Another", SprintStatus::Planned, NOW).expect("a sprint");
        let elsewhere = store
            .create_task(NewTask { title: "Elsewhere".to_owned(), sprint_id: Some(other.id), ..NewTask::default() }, NOW)
            .expect("a ticket");
        let first = store.task_by_key("task-1").expect("a read").expect("task-1");
        let second = store.task_by_key("task-2").expect("a read").expect("task-2");
        store.move_task(first.id, Status::InProgress, 0, NOW).expect("a move");
        store.move_task(second.id, Status::InProgress, 1, NOW).expect("a move");
        store.claim(second.id, "a-session", Assignee::Claude, "pid:1", NOW).expect("a claim");
        store.reclaim(second.id, LATER).expect("a reclaim");
        let read = store.board().expect("the board");
        let new_lane = read.lane(Status::New).expect("New");
        assert_eq!(
            new_lane.tasks.last().expect("the reclaimed card").key,
            "task-2",
            "it goes to the foot of New rather than to whichever position it happened to hold"
        );
        assert_eq!(
            new_lane.tasks.iter().map(|task| task.position).collect::<Vec<i64>>(),
            (0..new_lane.count() as i64).collect::<Vec<i64>>(),
            "New's positions are contiguous"
        );
        let in_progress = read.lane(Status::InProgress).expect("In Progress");
        assert_eq!(
            in_progress.tasks.iter().map(|task| task.position).collect::<Vec<i64>>(),
            [0],
            "and the lane it left has no hole where it was"
        );
        assert_eq!(
            store.task(elsewhere.id).expect("a read").expect("elsewhere").position,
            0,
            "a ticket in another sprint was not renumbered"
        );
        let _ = sprint;
    }

    #[test]
    fn an_edit_leaves_alone_every_field_it_was_not_given() {
        let (store, _) = filled();
        let task = store.task_by_key("task-1").expect("a read").expect("task-1");
        store
            .edit_task(
                task.id,
                &TaskEdit { title: Some("Renamed".to_owned()), ..TaskEdit::default() },
                LATER,
            )
            .expect("an edit");
        let read = store.task(task.id).expect("a read").expect("task-1");
        assert_eq!(read.title, "Renamed");
        assert_eq!(read.priority, task.priority, "the priority was not given, so it did not change");
        assert_eq!(read.assignee, task.assignee);
        assert_eq!(read.description, task.description);
        assert_eq!(read.updated_at, LATER);
    }

    #[test]
    fn an_epic_keeps_its_colour_and_a_card_can_name_it() {
        let (store, sprint) = filled();
        let epic = store.create_epic("Plugins", "#7F5AF0").expect("an epic");
        let task = store
            .create_task(
                NewTask { title: "A".to_owned(), epic_id: Some(epic.id), sprint_id: Some(sprint), ..NewTask::default() },
                NOW,
            )
            .expect("a ticket");
        let read = store.board().expect("the board");
        assert_eq!(read.epic(epic.id).expect("the epic").color, "#7F5AF0");
        assert_eq!(
            read.tasks().find(|card| card.id == task.id).expect("the card").epic_id,
            Some(epic.id)
        );
    }
}
