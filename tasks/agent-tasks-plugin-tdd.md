# agent-tasks-plugin — the task board as a Unluminate plugin

> For our next plugin, we want rebuild our entire Tasks project (terminals, tasks, jira, etc) as a
> plugin called Agent-Tasks in rust. The db should be a built in sql-lite db that is stored somewhere
> appropriate and shown as a configuration.
>
> The UI should should be laid out exactly like ours, have same run features, terminal chat, resume old
> tasks sessions, etc.
>
> Do a full survey and TDD of this tasks project features. Understand fully all functionality, and have
> tests which confirm it works as expected.
>
> The design should look like our current, but dark theme. The background can match the transparency of
> our IDE settings.

The board, its tickets, its terminals and its agents, drawn in `egui` inside Unluminate, over a single
SQLite file. `services/agent_tasks/` is the implementation and this is the design.

## 1. What is being replaced

Tasks and Remote Control is a local task board whose tickets are worked by Claude or Codex in durable
pseudoterminals. Three clients attach to the same processes: a browser board at
`http://localhost:4310/tasks`, a Remote Control page for a phone, and a Tauri desktop shell named
Tasks that loads the same origin.

It runs as four processes.

| Process | What it owns |
|---|---|
| `tasks-terminal-daemon` on `ws://127.0.0.1:4312` | The real `node-pty` processes, one per session, with an `@xterm/headless` screen behind each for replay. It outlives the API, which is what makes a terminal survive an API restart. |
| `tasks-api` on `http://127.0.0.1:4311` | A NestJS application: 12 controllers, the board's event stream, the watchdog, the JIRA sync, the scheduler, Slack, and a socket.io gateway that bridges browsers to the daemon. |
| `tasks-ui` on `http://localhost:4310` | A Next.js application: the board, the ticket detail, the Remote Control page. |
| PostgreSQL on `127.0.0.1:5433` | Database `tasks_remote_control`, schema `tasks_remote`, 10 tables. |

Everything below is measured from the code rather than remembered.

### 1.1 The data

Ten tables. `task` carries 27 columns.

| Table | What it holds |
|---|---|
| `task_epic` | Name, colour, position. A card's coloured left edge and its chip. |
| `sprint` | Name, status of `planned`, `active` or `completed`, start and end dates, position. |
| `task` | The ticket. Key, title, description, priority, status, assignee, model, effort, epic, sprint, position, source path, owner, the agent columns, the lease columns, the watchdog counters, the JIRA columns, the deep research pair, and the two timestamps. |
| `task_todo` | Text, done, position, per task. |
| `task_comment` | Author of `jason`, `claude`, `codex` or `system`, body, and a JIRA comment id. |
| `agent_workspace` | Per project and agent kind: the open terminals, the open files and the active tab, as JSON. |
| `agent_project` | Id, name and directory of a project an agent can be launched in. |
| `task_schedule` | A cron expression, a timezone, a command, and the last and next run. |
| `slack_chat_conversation` | One conversation per Slack thread, with its items and usage as JSON. |
| `auth_session` | The browser's session cookie. |

Four constraints are the board's rules written into the database, and the plugin keeps all four:
`status` is `new`, `in_progress`, `agent_done` or `qa_failed`; `priority` is `low`, `medium` or `high`;
`assignee` is `claude`, `codex` or `jason`; and `sprint.status` is `planned`, `active` or `completed`.

Schema changes are additive. `ensureTablesExist` runs `CREATE TABLE IF NOT EXISTS` and then a second
pass of `ADD COLUMN IF NOT EXISTS`, and nothing is ever dropped.

### 1.2 The board a person sees

A left rail of seven entries: Board, Messages, Backlog, Completed, Epics, Schedule, and a link to
Remote Control. It expands to show its labels and remembers which state it was in.

A header carrying `Current Sprint`, the sprint's name, a three way origin filter for tickets created
locally, tickets synced from JIRA and both, a search box, a `Sync JIRA` button and `Add Task`.

Four lanes across: `NEW`, `QA FAILED`, `IN PROGRESS`, `AGENT DONE`, each with a coloured dot, its name
and a count. Cards are dragged between them.

A card shows the title, the epic chip in the epic's colour, the JIRA key as a link, a priority mark of
one of three chevrons, the ticket key, the completed and total todo counts, the comment count, a deep
research button with three states, a start button, and a round agent badge that is brighter while a
terminal is attached.

The ticket detail holds the title, a markdown description editor with write, split and preview modes
and image paste, the todos, the comments, the ticket's own terminal with `Resume session`, and
`Re-sync from JIRA`.

`+ Add Task` creates the row immediately and opens an editor that saves as you type, so the ticket
exists from the moment the editor opens.

### 1.3 What happens when work starts

Pressing start on a card launches an agent in a pseudoterminal in the ticket's project directory, and
writes `agent_session_id` on the row. Claude is invoked as `claude --dangerously-skip-permissions
--model <model> --effort <effort> --session-id <uuid>`, or `--resume <uuid>` when a session is being
resumed. Codex is invoked with `--model` and `-c model_reasoning_effort`, where `xhigh` and `max`
collapse onto `high` because Codex recognises nothing above it. Once the agent is ready for input, the
handoff line `/task begin task-N` is typed into it.

`Resume session` brings back a terminal that was retired, without changing which lane the ticket is in.
Sending a comment to the terminal resumes a retired session first, so nothing is typed at a dead
process. Moving a ticket from `agent_done` back to `qa_failed` resumes the session on its own.

### 1.4 The watchdog

One tick every two minutes over cards in `in_progress` that recorded an agent session, and it separates
two failures.

A worker that is **gone** is a card whose lease expired and whose pseudoterminal is no longer running.
There is nobody to talk to, so the card is struck once per tick, the first strike posts a comment, and
after `WATCHDOG_STRIKES_BEFORE_RECLAIM` strikes the card returns to `new` with its todos and comments
intact.

A worker that has **stopped** is a card whose terminal is still running while the agent inside it waits
at its prompt. That agent can be talked to, so a continue instruction is typed into its terminal instead
of the ticket being taken away. Two things mark an agent as stopped: the lease expired, or the terminal
printed nothing for `WATCHDOG_SILENT_MINUTES`. The instruction repeats no more often than
`WATCHDOG_NUDGE_INTERVAL_MINUTES`, and from nudge `WATCHDOG_NUDGES_BEFORE_BLOCK` on it also tells the
agent how to end a ticket it cannot finish: a comment whose first word is `Blocked`, then `agent_done`.

Only board activity stops the nudges. A todo, a comment or a heartbeat clears the count; terminal output
does not, because the nudge itself is echoed by the terminal it was typed into. A paused agent clears
both counters, because a stopped process cannot answer and its silence means nothing.

### 1.5 The JIRA sync

One direction only. Nothing is ever written back to JIRA. Every 15 minutes and on the button, using
`assignee = currentUser() AND sprint in openSprints() ORDER BY created DESC`. A second pass then fetches
every JIRA key already on the board that the search did not return, one request each, so a ticket whose
sprint closed or which was reassigned still shows current JIRA.

JIRA owns the lane on every sync: `Done` or the `done` status category goes to Agent Done, `In Progress`
or the `indeterminate` category to In Progress, `To Do` or the `new` category to New. The status name is
matched before the category. Two cards are exempt: a card in In Progress that recorded an agent session,
and a card in QA Failed, which is a human's verdict JIRA has no status for.

`position`, `assignee`, `model`, `effort`, `sprintId` and `agentProjectId` are set once at creation and
left alone. Title, description, priority and the JIRA metadata are refreshed on every sync. Descriptions
and comments are converted from Atlassian Document Format to markdown.

### 1.6 What the plugin does not reproduce

Three parts of Tasks and Remote Control are deliberately not in the plugin, and §11 gives the reason for
each: Slack and Slack chat, the Remote Control page for a phone, and the browser login. They are
features of a server that serves other clients, and a plugin inside one window is not that.

## 2. What was weighed

### 2.1 Wrapping the running services, against owning the data

The plugin could have been a client of the API that already exists. It is running on this machine, it
holds the data, and it owns the terminals.

That was refused, and the ticket asks for the opposite: a built in SQLite database. Three reasons hold
independently of the ask. A pane that draws nothing until four processes are up is a pane that is empty
most of the time, and the failure looks like a bug in Unluminate. A plugin that needs PostgreSQL, Node and a
daemon is not a plugin somebody installs. And Unluminate already owns real pseudoterminals with a real
emulator in `unluminate-terminal`, so going through a WebSocket to a daemon that spawns `node-pty` would be
a second terminal stack inside a program that has one.

What is lost is stated rather than hidden: **the plugin's board is a different board from the browser's.**
They do not share a database and nothing syncs between them. §11 records what a bridge would take.

### 2.2 SQLite, against the two alternatives

| | Verdict |
|---|---|
| **`rusqlite` with the `bundled` feature** | **Chosen.** The C library is compiled into Unluminate, so there is nothing to install and no server to be running. One file, which is what makes the setting in §4 meaningful and the tests disposable. Transactions, indexes and a real query language, which the board's queries need: the lanes are one grouped read and the watchdog's candidate query is arithmetic over timestamps. |
| **PostgreSQL** | What is being replaced. A server per machine, a role, a schema and a port, and the failure mode the environment notes in the repository being replaced already record: the Docker server on 5432 stores its data on tmpfs, so every restart destroyed the database. A text editor does not ask for a database server. |
| **A file of records Unluminate writes itself** | The board has ten tables, foreign keys with cascade deletes, ordering within lanes, and a search across two text columns. `settings.conf` is `name = value` and needs no parser worth a dependency; this needs joins. Writing the storage engine instead of using one is the mistake `pulldown-cmark` was refused for in reverse. |

`rusqlite = { version = "0.32", features = ["bundled"] }` adds one dependency and a C compile. Nothing
is fetched at run time, which is the rule the whole repository keeps.

### 2.3 The terminal

`unluminate-terminal` already has everything the board needs and it is already tested with no window: a
session over a pseudoterminal, the screen a painter reads, the colour palette, key encoding, mouse
reports, and `Tabs` holding several sessions with one active.

So a ticket's terminal is a `unluminate_terminal::Session` owned by the plugin, and the pane draws it with
the same painter the terminal tile uses. There is no daemon, no WebSocket, and no second emulator.

The cost is the one thing the daemon bought: **a session does not survive Unluminate closing.** The daemon
was a separate process precisely so that restarting the API left the terminals alive. §5.4 is what
replaces it, and it is the mechanism the agents themselves already rely on: Claude and Codex both resume
a conversation by id, so what has to survive is the id rather than the process.

## 3. Where the plugin sits

Agent-Tasks is a `ui` plugin, and every contribution in §4 of the UI plugin architecture design is one
it uses. That is deliberate: the first UI plugin exercises every contribution, so no contribution ships
without a customer.

```
plugin.kind = ui
ui.provider = agent-tasks

pane.id     = board        # the rail button, and the pane it opens
tab.id      = board        # the same board as a tab in the editing area
menu.name   = Agent-Tasks  # with entries and a nested submenu
settings.page = Agent-Tasks
```

The layout is one crate module with one file per part of the window, which is `unluminate-app`'s own rule.

```
services/agent_tasks/
  mod.rs          the provider: the trait implementation, and the state it owns
  store.rs        the SQLite file: schema, migration, and every query
  model.rs        Task, Todo, Comment, Sprint, Epic, Lane, Status, Priority, Assignee
  board.rs        the lanes: which cards are in which lane, in what order, and the drag
  agent.rs        launching Claude or Codex, the handoff line, and resuming a session
  watchdog.rs     the two failures and the five thresholds, with no clock of its own
  search.rs       the search box
components/agent_tasks/
  board_view.rs   the header, the lanes and the cards
  card.rs         one card
  detail.rs       the ticket detail: description, todos, comments, terminal
  settings.rs     the Settings page
```

`store.rs`, `model.rs`, `board.rs`, `watchdog.rs` and `search.rs` have no user interface dependency and
are tested with no window, which is the rule the crate table in `CLAUDE.md` sets for `unluminate-core`,
`unluminate-terminal` and `unluminate-git`. The four files under `components/` draw and decide nothing.

## 4. The database, and the setting that names it

One file, in the folder Unluminate already keeps its settings in.

```
macOS    ~/Library/Application Support/Unluminate/plugins/agent-tasks/board.sqlite3
Windows  %APPDATA%\Unluminate\plugins\agent-tasks\board.sqlite3
Linux    ~/.config/Unluminate/plugins/agent-tasks/board.sqlite3
```

`store::default_path` is that path and `Settings -> Agent-Tasks` shows it in a field with `Reveal` and
`Change`. The path is a setting rather than a constant because the ticket asks for it, and because the
one thing a person will want to do with a board they care about is put it somewhere that is backed up.

The setting lives in `plugins/agent-tasks/settings.conf` beside the manifest, in the same
`name = value` format, read by the same `store::Values`. Four values:

```
database   = <path>            # empty means the default above
project     = tasks-and-remote  # the project a ticket's agent is launched in by default
agent       = claude            # claude or codex
lease       = 45                # minutes before the watchdog calls a lease expired
```

**The schema is the ten tables of §1.1, with three deliberate differences.**

`SERIAL` becomes `INTEGER PRIMARY KEY AUTOINCREMENT`, `TIMESTAMPTZ` becomes `TEXT` holding an ISO 8601
instant in UTC, and `JSONB` becomes `TEXT` holding JSON. SQLite has no `now()` default that returns an
instant with an offset, so the store writes the timestamp, which also makes every test able to hand in
a fixed clock.

`auth_session` is dropped. There is no browser and no cookie, and a table for a login that does not
exist would be a table nothing ever writes.

`slack_chat_conversation` is dropped. §11.

**Migration is additive and keeps the shape the application being replaced chose.** `store::open` runs
`CREATE TABLE IF NOT EXISTS` for every table and then a pass that adds any column a later version needs,
guarded by a read of `pragma_table_info`, because SQLite has no `ADD COLUMN IF NOT EXISTS`. Nothing is
ever dropped and no table is ever recreated. A `schema_version` row in a `meta` table records what the
file was last written by, so a file from a newer Unluminate is refused with a message rather than opened and
half understood.

**Every query is a named function on `Store` and there is no query anywhere else.** That is what makes
the store testable and what stops the drawing code holding SQL. Twenty six functions, and the ones worth
naming here are `board`, which is one read per lane ordered by position; `task_by_key`; `save_task`;
`add_todo`, `set_todo_done` and `reorder_todos`; `add_comment`; `heartbeat`; `claim`, which is the
guarded update that moves a card to `in_progress` only if it is not already claimed; and
`watchdog_candidates`, which is the arithmetic over `heartbeat_at` and `lease_duration_minutes` that
§1.4 describes.

## 5. What the plugin does

### 5.1 The board

Four lanes, drawn as columns, in the order the design image shows: `NEW`, `QA FAILED`, `IN PROGRESS`,
`AGENT DONE`. Each has a coloured dot, its name in the dim text colour, and a count on the right. Cards
are dragged between lanes and reordered inside one, which is `position` on the row, and the drag reuses
`file_tabs::Strip::position_at`'s rule: a card goes after every card whose middle the pointer has passed.

The header holds `Current Sprint`, the active sprint's name, a search box, and `Add Task`. The three way
origin filter and `Sync JIRA` are drawn only when JIRA is configured, which is the rule that a control
that cannot apply is absent.

The card is §1.2's card: title, epic chip, JIRA key, priority chevron, key, todo counts, comment count,
the start button and the agent badge. The badge is brighter while a session is attached, which is the
one piece of state on the board that changes without anybody pressing anything.

### 5.2 The ticket detail

The detail is not a modal. It fills the pane, with a back arrow, because the pane can be 420 points wide
on the right hand side of the window and a modal in a 420 point column is a modal wider than its parent.
In the tab, where there is a whole editing area, the board is on the left and the detail on the right,
which is the arrangement the Markdown side by side view already uses.

The description is markdown, and Unluminate already reads and draws markdown. So the description is a
`unluminate_core::Document` and the preview is the one `components::editor_view` draws, which means the
description editor is the editor, with the same three view modes, the same fonts and the same code
blocks. That is the largest single saving in the design and it falls out of the plugin being inside a
text editor.

Todos are rows with a tick box, dragged to reorder. Comments are markdown, newest last, each with its
author and how long ago. `Post comment` writes one, and `Send to terminal` types it into the ticket's
agent.

### 5.3 Running an agent

`agent.rs` builds the same command lines §1.3 records, from the same three registries the rest of Unluminate
uses for this kind of decision: which agent programs exist is a list in Unluminate, which one a ticket uses
is data on the row, and where it is found on this machine is `services::debuggers`' pattern applied to
agents.

```rust
/// The agents this version of Unluminate can launch. Checked the way `plugins::DEBUGGERS` is checked.
pub const AGENTS: &[&str] = &["claude", "codex"];
```

Launching one is `unluminate_terminal::Session::spawn` in the ticket's project directory, then waiting until
the agent is ready for input, then writing the handoff line. Claude is ready after 1800 ms, which is what
the application being replaced measured; Codex is ready when its prompt appears. Both numbers are in one
place with the measurement written beside them.

**The session id is a UUID Unluminate chooses for Claude, and Codex does not take one.** Claude accepts
`--session-id <uuid>` on a first run and `--resume <uuid>` later, so the id stored on the row is one
Claude will answer to. `codex --help` offers no equivalent: a Codex session gets an id from Codex and
`codex resume <id>` takes it back, so a first Codex run names nothing.

The id is still written on a Codex ticket, because the watchdog's whole question is whether a card has a
worker at all, and that marker is what answers it. What it cannot do is name a conversation, so
`Resume session` on a Codex ticket refuses with a sentence saying that Codex names its own sessions and
that pressing Start begins a new one which reads the ticket's comments. Capturing the id Codex wrote —
which is what the board being replaced does, reading the rollout id recorded after the launch — is its own
piece of work, and §10 keeps it.

### 5.4 Resuming a session after Unluminate has closed

The daemon existed so that a terminal survived the API restarting. A plugin inside the window cannot
have that, so the design keeps the thing that actually matters: **the conversation, not the process.**

`agent_session_id` is on the row. When the board is opened and a card in `in_progress` has one, the card
shows the agent badge dim rather than bright, and for a Claude ticket `Resume session` starts a new
pseudoterminal with `--resume <that id>`. The agent comes back with its whole conversation, in the same
project, and the first thing it is told is to re-read the ticket, which is what the task protocol already
requires of a resumed run.

So for Claude, `Resume session` means the same thing it means today from a person's point of view, and it
is implemented by the mechanism the agent already has rather than by a process that outlives the editor. A
session started in one Unluminate is resumed by another Unluminate window, because the id is in the database rather
than in the window.

**For Codex it is refused, and the refusal is the honest answer.** Codex names its own sessions, so the id
on the ticket is Unluminate's marker rather than a conversation Codex has heard of. Pressing Start begins a new
one, and what it reads is the ticket: the description, the todos and the comments, which is what the
previous worker left behind for exactly this. Building the other half — finding the rollout id Codex wrote
after the launch — is §10.

### 5.5 The watchdog

`watchdog.rs` is the two failures of §1.4 and the five thresholds, and it has **no clock and no timer**.
`Watchdog::tick(now, candidates, terminals) -> Vec<Action>` is a pure function: it is handed the instant,
the rows the store's candidate query returned, and which sessions are alive and which are paused, and it
returns what to do. The window calls it every two minutes from the same place it already asks git for the
status.

That is what makes every rule in §1.4 a unit test with no window, no database and no clock: a lease that
expired with a dead terminal strikes, the third strike returns the card to `new`, a live terminal is
nudged instead of struck, a nudge is not repeated inside the interval, the escalated nudge names the
`Blocked` ending, a paused session clears both counters, and terminal output does not clear them while a
comment does.

### 5.6 JIRA

The sync of §1.5, with the same query, the same two passes, the same lane rules and the same exemptions.
Reading JIRA is HTTP and Unluminate has no HTTP client, so this is the one part of the plugin that adds a
dependency for a network call: `ureq`, blocking, on the worker thread pattern `unluminate_git::Worker`
already establishes. **The rule that nothing is fetched is not broken by it.** That rule is about the
editor reaching out on its own: a document cannot make a request, a diagram fetches nothing, and no
adapter is downloaded. A sync a person configured with their own credentials and asked for with a button
is the same category as the terminal running the command they typed.

Credentials are read from the machine's keychain by the same three service names the application being
replaced uses, and never from a file in the plugin. With none configured, every JIRA control is absent.

### 5.7 The other four views

`Messages` is Slack and is not here. The remaining four rail entries of §1.2 are: `Backlog`, tickets with
no sprint; `Completed`, tickets in closed sprints; `Epics`, the epic list with its colours; and
`Schedule`, the cron rows of `task_schedule` with their next run. Each is a section of the same pane
chosen by a row of tabs across the top, rather than four panes, because four rail buttons for one plugin
would fill the rail.

The scheduler that runs them is the watchdog's shape again: a pure function handed the instant and the
rows, returning which schedules are due.

## 6. The look

The ticket says the design should look like the current board but dark, and that the background can match
the transparency of the IDE settings. Both fall out of the `Look` value the provider is handed, which is
§5 of the UI plugin architecture design.

**Every colour comes from `theme::color`.** The mapping is written down once, here, so that a reader can
check a drawn card against the palette:

| What | Colour |
|---|---|
| The pane's background | `EDITOR`, with the opacity setting applied, so the desktop shows through the board exactly as it shows through the text |
| A lane's background | `EXPLORER`, which is the panel ground every list in Unluminate sits on |
| A card | `CONTROL`, with `CONTROL_BORDER` round it and the epic's colour down its left edge |
| A card under the pointer | `SELECTED_ROW`, which is the pill every list in Unluminate draws for its chosen row |
| A card's title | `TEXT_STRONG`; its key, counts and dates `TEXT_DIM` |
| The lane names and counts | `TEXT_DIM` |
| The start button | `ACCENT` |
| The lane dots | `TEXT_DIM` for New, `GIT_MODIFIED` for In Progress, `UNSAVED` for QA Failed, `GIT_ADDED` for Agent Done |
| The high, medium and low priority chevrons | `UNSAVED`, `TEXT_CONTROL`, `TEXT_DIM` |

The epic's own colour is the one colour on the board that comes from the data rather than the palette,
and it is confined to the left edge of the card and the chip, which is what the board being replaced
does. That is the same allowance the file icons already have.

**Every measurement comes from `theme::size` and the style guide.** A card row is 28 points, a menu row
is 24, a lane is 300 wide at a minimum, and the corner radius is the one every control uses. The fonts are
the settings' family and size, and the terminal inside a ticket uses the terminal font size, so changing
either in `Settings -> Appearance` changes the board in the same frame.

## 7. Reaching all of it without a pointer

Every control on the board is reachable from `unluminate-cli plugin run agent-tasks <command>`, and
`plugin view agent-tasks` prints the board as JSON. Nineteen commands, one per thing a person can do:

```
board                       the lanes, their counts and their cards
task <key>                  one ticket with its todos and comments
new-task --title --lane     create
edit-task <key> --title --description --priority --assignee --model --effort --epic
move-task <key> --lane --position
delete-task <key>
todo-add <key> --text        todo-done <key> <n>        todo-remove <key> <n>
comment <key> --body         comment-send <key> --body   sends it to the ticket's agent
start <key>                 launch the agent and hand off
resume <key>                resume the recorded session
interrupt <key>             stop <key>
heartbeat <key> --minutes
sync                        run the JIRA sync now
search --query
```

That list is not a convenience. A board drawn with `egui` is invisible to a test and to an agent, and
Unluminate's rule is that everything a person can do an agent can do too, through the same code. So each of
those goes into `provider.command`, which is the same function the button calls, and the answer to
`plugin view` is built from the same store reads the drawing uses.

## 8. Tests

The application being replaced has 276 test cases on its API and 25 in Playwright. The plugin's tests are
in the same places Unluminate's are, and the count below is what the design commits to.

**The store, with no window and a temporary file.** Opening a file that does not exist creates the schema.
Opening it again is a no operation. A file written by a newer schema version is refused with a message. A
task with no key gets one. The four check constraints are enforced. Deleting a task deletes its todos and
comments and nothing else. Reordering inside a lane keeps positions contiguous. `claim` moves a card to
`in_progress` once and refuses the second caller. `board` returns the lanes in order with each lane's
cards in position order. `watchdog_candidates` returns only cards in `in_progress` with a session, and
computes idle minutes and lease expiry from the instant it is handed.

**The model, with no window.** Every status, priority and assignee round trips through its string. An
unknown value from the file is refused rather than defaulted, because a row nobody can explain is worse
than an error.

**The board arithmetic, with no window.** Which lane a card is in, the drag's landing position, the
search's matching, and the counts on a card.

**The watchdog, with no window, no database and no clock.** The eight rules of §5.5, each as one case
handing in a fixed instant.

**The agent command lines, with no process.** Claude's first run and its resume, Codex's `-c
model_reasoning_effort`, `xhigh` and `max` collapsing onto `high`, and the handoff line for each.

**The JIRA mapping, with no network.** The three lane rules, the name matched before the category, the
two exempt cards, which fields are refreshed and which are set once, and Atlassian Document Format
converted to markdown.

**The window, through the real widget tree with `egui_kittest`.** The rail button opens the board. The
board shows four lanes. `Add Task` creates a card and opens its detail. Typing in the title changes the
card. Pressing a todo's tick box changes the count on the card. Dragging a card to another lane changes
its status in the store. The detail's terminal draws a grid. The plugin's Settings page shows the
database path.

**Screenshots**, accepted after somebody has looked at the image: the board in the pane docked right, the
board as a tab filling the editing area, a ticket's detail with its todos and comments, a ticket's
terminal running, and the Settings page. Each is compared against the design image the ticket carries.

**The command line.** Each of the 19 commands driven against a real window, with the answer checked
against the store read back rather than against what the command said it did.

**A real agent, once.** One test that is not run in the ordinary suite and is marked so: launch Claude
against a scratch board, hand off a ticket, and assert the board records a claim, a todo and a comment.
It is the equivalent of the debugger tests that drive a real lldb, and it is the only way to know the
handoff line still works.

## 9. What is measured rather than assumed

`cargo run --release -p unluminate-app --example board_cost` is what measures all of it, on a board of 5000
tickets — more than any board anybody keeps, which is the point of the number.

**Drawing the board costs nothing when nothing changed.** The store is read when the board opens, when a
command changes something, and when the two minute tick fires, and never once a frame. The cards are held
in memory and a frame in which nothing changed does no work at all, which is the rule `symbols::Hover`
and the completion popup already keep.

**Opening the board is 27.50 ms with 5000 tickets**, worst of twenty reads. One query for the cards
ordered by status and position, plus the sprint and the epics, with an index on `status` and on
`sprint_id` — the two indexes the schema being replaced already has. Four round trips to answer one
screen would be three more than the question needs.

**One ticket with its todos and its comments is 0.16 ms**, which is what opening a card costs.

**One watchdog tick is 0.35 ms.** It reads only cards in progress that recorded a session, so on a board
where nothing has been launched it costs the index scan and returns nothing.

**The search is 31.14 ms at worst on 5000 tickets**, and that is the one number here that is worth
arguing about. It is `LIKE` with the query between two wildcards over the key, the title and the
description, so it reads every row's description, and a query matching a hundred tickets is the worst
case measured. A full text index would answer in under a millisecond and would be a second copy of every
description to keep in step, which is the one cost a search never pays today: nothing to invalidate when
a ticket is edited. 31 ms is under two frames at 60 Hz and the search runs when a key is pressed in the
search box rather than once a frame, so it is paid where a person is already waiting for the answer. It
is the number to watch: at ten times the tickets it would need the index, and the shape of that change is
one more table and one more trigger, which is why the store's `search` is a named function in one file.

**The terminal costs what the terminal tile costs.** It is the same session and the same painter, so there
is no new number.

## 10. Deliberately not here

**Slack and Slack chat.** Four of the application's 71 source files are the Slack client, and 12 more are
the Slack chat conversation: a Claude conversation per Slack thread, streamed, with its transcript stored
as JSON. That is a chat product that happens to live in the same repository as a board. It needs a Slack
application, a login flow, a token in the keychain and a WebSocket to Slack, and none of it is about
tickets. The `Messages` rail entry is absent rather than empty.

**The Remote Control page.** A phone attaching to a terminal is the reason the daemon is a separate
process and the reason there is an HTTP server at all. A plugin inside one window has no way to serve a
phone, and giving it one would mean putting a web server inside Unluminate.

**The browser login.** No browser, no cookie, no session table. The board is a file on this machine
readable by whoever can read the file.

**Two boards kept in step.** The plugin's board and the browser's board are different boards. A bridge
would mean one of them owning the data and the other becoming a client, which is §2.1's refused design
seen from the other end. If it is ever wanted, the shape is clear: the store gains a second
implementation that speaks to the API, and everything above it is unchanged, because every query is
already a named function in one file.

**The desktop shell.** Tasks.app exists so the browser board has a dock icon. The plugin is inside an
application that already has one.

**Dictation.** The description editor can dictate through a local speech service the API proxies. Unluminate
has no audio and adding one for a text field is a feature of its own.

**Image paste into a description.** The board being replaced writes a pasted image into the project's
uploads folder and rewrites the markdown to point at a route that reads it back. Unluminate draws pictures
already and `services::preview_images` already resolves an image in a markdown preview, so the pieces are
there; what is missing is a decision about where a pasted image is written, and it is not a decision this
design needs to make to draw a board.

**The supervisor.** Draining the whole sprint with one agent is a second protocol, and it belongs with the
one command that starts it rather than with the board.

**Resuming a Codex conversation.** Codex assigns its own session id, and finding the one it wrote means
reading the rollout Codex recorded after the launch and matching it by time, which is what the board being
replaced does. It is a piece of work with its own failure modes — two agents launched in the same second,
a rollout Codex has not flushed yet — and it is not the difference between a board that works and one that
does not: a Codex ticket is started again and reads its own comments. §5.4 says what happens instead, and
`Resume session` on a Codex ticket says it out loud rather than quietly starting something new.
