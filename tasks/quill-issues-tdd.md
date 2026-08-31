# quill-issues — the reported faults, their causes, and what each change is

`task-28` on the local board reports twelve things across the explorer, the Agent-Tasks plugin, its
Settings page, the terminal and the tests. This document is the specification for all of them. It is
written before the code because half of these are not gaps to fill but faults whose cause has to be
named first: two of them freeze the window, and one of them stops an agent being launched at all.

Every issue below is a section, and every section says the same four things: what was reported, what
is actually happening in the code, what changes, and what proves it. Quill's own rule from
`CLAUDE.md` applies to each of them — a control a person uses, the same code reached by an agent, and
tests over both.

## 0. The state this starts from

The Agent-Tasks plugin has never been committed. `git status` on `main` lists 69 changed or new paths
and every file under `crates/quill-app/src/services/agent_tasks/`,
`crates/quill-app/src/components/agent_tasks/` and `crates/quill-app/plugins/agent-tasks/` is
untracked. So none of what follows is a change to a released feature, with one exception that matters
a great deal: the board **database** on this machine is real, has been written to, and is what proves
the migration fault in §5.

`cargo check --workspace --all-targets` passes before any of this, so a failure afterwards belongs to
this work.

---

## 1. The explorer freezes when a folder with a lot of files is opened

> When I try to expand a folder with a lot of files, like when quill is at /, and I expand dev, quill
> freezes and I have to force quit.

This is the most serious item on the ticket, it is three separate faults, and all three are on the
thread that draws the window.

### 1.1 Reading a folder opens and reads every file in it

`services::file_tree::read_directory` builds an `Entry` for each child, and `Entry::new` calls
`file_kind::openable` on every file so the row can be drawn dimmed when Quill cannot open it.
`file_kind::openable` does two things per file. It calls `std::fs::metadata`, which is a second `stat`
on a path the directory read has already described. Then it calls `is_text`, and `is_text` answers
from the extension for a known extension and otherwise falls through to `looks_like_text`, which
**opens the file and reads the first few thousand bytes**.

So opening a folder of twenty thousand files with no extension is twenty thousand `open` and `read`
pairs before one row is drawn. That is the slow case. The hanging case is worse and is what the
ticket describes: under `/dev` the entries are character devices, block devices and FIFOs, and
`File::open` followed by `read` on a FIFO with no writer **blocks for ever**. The window is inside
`FileTree::toggle` at that moment, so there is nothing to interrupt and nothing to draw. Force
quitting is the only way out, which is exactly what was reported.

`file_kind`'s own documentation already says this must not happen. `is_image` carries the comment
"the explorer asks this of every row, so it must not read the file". `openable` is asked of every row
too, and it does read the file.

**The change.** The explorer decides openability from the name and the kind of directory entry alone,
and never from the contents.

- `read_directory` reads `entry.file_type()` once, which it already has, and keeps three cases rather
  than two: a directory, a regular file, and anything else. Anything else — a device, a FIFO, a
  socket — is listed, because the explorer is a picture of the folder, and is not openable, with no
  I/O of any kind.
- `Entry::new` takes the kind it was given rather than asking the file system again, so the second
  `stat` per child is gone.
- A new `file_kind::openable_by_name` answers from the extension, the known bare names and the
  `.gitignore` shape, and returns `Ok` for an unknown extension rather than reading. An unknown
  extension is *offered*, and the definitive answer happens when the file is opened, where a tab can
  say what went wrong. That is the same bargain `is_image` already makes for a `.png` that is not a
  PNG.
- `file_kind::openable` keeps its reading behaviour for the one caller that should have it: opening a
  file. Nothing on a drawing path calls it.

The size refusal survives: a regular file's length comes from the metadata the directory entry
carries on every platform Quill runs on, so `Refusal::TooLarge` is still decided without a second
`stat`.

### 1.2 Every row in the tree is drawn every frame

`components::explorer` calls `tree.rows()`, which allocates a `Vec` holding one `Row` for the whole
tree, and then draws all of them. Each row allocates its own rectangle and interacts. A tree with
twenty thousand rows opened out is twenty thousand widgets a frame, and egui charges for every one of
them whether or not it is on screen. So even once the read in §1.1 finishes, the window stays
unusable while that folder is open.

**The change.** The explorer draws only the rows inside the visible rectangle. Every row is
`size::ROW` tall and that is fixed, so the first and last visible rows are arithmetic rather than a
measurement: the scroll offset divided by the row height, and the height of the list divided by the
row height. Space is added above the first drawn row and below the last so the scroll bar still
describes the whole tree. `tree.rows()` is unchanged and still cheap relative to drawing, and the
count it answers with is what the space above and below is computed from.

### 1.3 Filtering reads up to three hundred files a frame

In the filter branch the explorer calls `file_kind::openable(path)` on every match, once a frame. The
filter reports up to `SEARCH_LIMIT` of them, which is 300. That is up to 300 file opens and reads per
frame while somebody is typing a filter, on the drawing thread.

**The change.** The filter branch calls `openable_by_name` from §1.1, which reads nothing.

### 1.4 What proves it

With no window:

- `read_directory` on a folder holding a FIFO returns, and the FIFO's entry is present and not
  openable. This is the test that would have hung before the change, so it is the one that matters
  most. It is skipped on Windows, which has no `mkfifo`.
- `read_directory` on a folder of two thousand extensionless files completes inside a budget, asserted
  as elapsed time against a generous ceiling, because a test that asserts "no file was opened" cannot
  see a syscall. The budget is deliberately loose: the fault it guards against is three orders of
  magnitude, not a percentage.
- `openable_by_name` agrees with `openable` for every extension in `TEXT_EXTENSIONS` and
  `BINARY_EXTENSIONS` and for the bare names, and differs only where the old one would have read the
  file.

Through the real widget tree:

- A tree with two thousand rows opened out draws a bounded number of row widgets, counted through the
  accessibility tree rather than asserted by eye.
- The rows on screen are the rows the scroll offset names: scrolling to the bottom of a long tree
  shows the last entry.

---

## 2. A toggle for the editing area, under the folder icon

> There should be a toggle on the left, under the folder icon, for file view. When that is pressed, it
> shows/hides the file pane on the right of folder pane (file pane has all the tabs).

The pane on the right of the explorer that holds all the tabs is what Quill calls the **editing
area** — `dock::Regions::editor`, with one `components::file_tabs` strip per pane inside it. The
ticket's "file pane" and Quill's "editing area" are the same thing, and the name in the window follows
Quill, because `panel list`, `dock` and a dozen accepted screenshots already use it.

The rail's top group holds `Project` then `Version Control`. The new button goes under them, which is
under the folder icon as asked.

**The change.**

- `Action::ToggleEditor`, on the `View` menu as `Hide Editor` and `Show Editor`, next to
  `Show Explorer`. A menu entry needs nothing else to be agent-reachable: `actions::menus` is walked
  to build `quill-cli action list`, so this is one entry rather than an entry and a catalogue row.
- A third button in the rail's top group, named `Editor`, drawn with `icon::file`. `RailState` gains
  `editor_visible`.
- `dock::regions` takes whether the editing area is showing. When it is not, `EDITOR_MIN_WIDTH` and
  `EDITOR_MIN_HEIGHT` are zero for the purpose of sharing the room out, so the panels take the whole
  width, and `Regions::editor` is `Rect::ZERO`. Nothing else in that function changes: the strips are
  still taken first and the columns still come out of what is left.
- `QuillApp::ui` draws no tab strip and no editing pane when the editing area is hidden.
- **Hiding the editing area cannot leave an empty window.** Hiding it while no panel is showing shows
  the explorer as well, and hiding the last panel while the editing area is hidden brings the editing
  area back. One rule stated twice rather than a refusal, because a button that sometimes does nothing
  is worse than a button that always leaves something to look at.
- Whether the editing area is showing is remembered per project by `services::project_state`, beside
  the panel sides and sizes it already writes.

**What proves it.** With no window: `dock::regions` gives the explorer the full width when the editing
area is hidden, and `Regions::editor` is `Rect::ZERO`; the two rules about an empty window hold in both
directions. Through the widget tree: the rail's `Editor` button hides the tab strip and the button goes
to its pressed state; `View -> Hide Editor` does the same thing; the project remembers it across a
reload. And `quill-cli action list` offers `Hide Editor`, which is the agent's way in.

---

## 3. Agent-Tasks opens in a tab, and the pane goes

> Let's just have agent-tasks be opened in a tab, rather than that other pane view.

The plugin contributes both a pane docked to the right and a tab, and the same board draws in both at
different sizes. The pane goes.

**The change.** `plugins/agent-tasks/plugin.conf` loses its whole `pane.*` block, and the menu's two
entries collapse to one: `open-tab=Open Board`. That is data, and it takes the rail button, the dock
slot, the pane's header and the sideways single-lane layout with it, because every one of those is
built from `plugins::Surfaces`.

The pane **machinery** in `app::plugin_panes`, `app::dock` and `components::activity_bar` stays. It is
a general capability of Quill's plugin system, described by `tasks/ui-plugin-architecture.md`, and no
plugin shipping a pane today is not a reason to delete the ability to. Its tests stop being about
Agent-Tasks and become about a manifest written for the test, which is a better test anyway: it stops
the pane machinery being verified only through one plugin's accidental use of it.

Two consequences in the plugin's own code. `components::agent_tasks::mod::pane` and the narrow layout
it chooses when `body.width() < 900.0` are no longer reachable from a pane, but the same narrow case
happens in a tab in a narrow window, so the code stays and is reached by making the window narrow.
And the plugin's `open-pane` command is removed from `commands()` and from the dispatch, so
`plugins run agent-tasks open-pane` answers that there is no such command rather than opening
something that no longer exists.

`crates/quill-app/tests/snapshots/agent_tasks_pane.png` stops being written by any test. It is left on
disk rather than deleted, and the operator is told it can go.

**What proves it.** The manifest reads back with no pane and one board entry on its menu; `panel list`
holds no plugin slot while Agent-Tasks is the only plugin that draws; `Open Board` opens a tab; and the
screenshot of the board as a tab is the one that is accepted.

---

## 4. Dropdowns for every value on a ticket

> Dropdowns. We need UI dropdowns for values.
> I'm unable to get an agent to do work because the model is a text field.

Two of the ticket's fields are free text where they should be a list, and five more are a wrapping row
of buttons rather than a list. `components::controls::dropdown` already exists — it is what the
toolbar and `Settings -> Appearance` use — and it is what these should all be.

**The change.** In `components::agent_tasks::ticket_modal`, and therefore in the new ticket editor
which is the same component in a different state:

| Field | Was | Becomes |
|---|---|---|
| Status | A row of four buttons | A dropdown of the four lanes |
| Assignee | A row of three buttons | A dropdown of claude, codex, human |
| Model | **A text field** | A dropdown of the models the chosen agent has, plus `the agent's default` |
| Effort | A row of five buttons | A dropdown of the five levels, plus `the agent's default` |
| Priority | A row of three buttons | A dropdown of low, medium, high |
| Epic | A row of buttons, one per epic | A dropdown of the epics, plus `None` |
| Project | **A text field** | A dropdown of the recent projects, plus this window's folder, and `Other…` which is still typed |

`choice_row` is replaced by a `dropdown_row` that takes the same `(value, said)` pairs it took, so the
call sites keep their shape and there is one place that draws a labelled dropdown rather than seven.
`Field` and `AgentTasks::edit_field` are untouched: every one of these still writes through the one
function, which is what §5.1 of `tasks/agent-tasks-ui-tdd.md` asks for.

**Which models exist is a list in Quill, and it is per agent.** A new `agent::MODELS` answers, keyed by
`Assignee`, the way `agent::AGENTS` already answers which agents there are. For Claude the entries are
the current Claude model identifiers; for Codex, the identifiers Codex takes. A ticket whose `model`
column holds something not in the list still draws that value as the chosen one and keeps it — a
board written by a newer Quill, or by hand, must not silently lose a model — so the list the dropdown
offers is the known models plus whatever this ticket already says.

`Project` is a dropdown over `services::store`'s recent projects, which is the list `File -> Open
Recent` already draws, because a project a person has opened is a project they might point a ticket at.
Anything else is still typable, since a ticket can name a folder this window has never opened.

The Settings page's `Default model` and `Default effort` become the same dropdowns, so the value a new
ticket gets and the value a ticket holds are chosen from one list.

**What proves it.** With no window: `agent::MODELS` has an entry for every agent in `AGENTS`, and a
value not in the list survives being drawn. Through the widget tree: each of the seven dropdowns is
opened, an option is chosen, and the row in the database holds it — one test per field, because a
single test over seven fields is a test that stops at the first one. The screenshots of the ticket
modal and the new ticket editor are retaken and looked at.

---

## 5. `Ticket 7 could not be claimed: no such column: owner`

> I see message at top right "Ticket 7 could not be claimed: no such column: owner".

This is a missing migration, and it is the reason no agent can be started on this board at all.

`store::SCHEMA` lists `owner TEXT` on the `task` table. Every table in `SCHEMA` is created with
`CREATE TABLE IF NOT EXISTS`, so a board file that already exists is left exactly as it is — a column
added to `SCHEMA` afterwards never appears in it. The additive migration that exists for precisely
this case, `store::migrate`, walks a list called `ADDITIONS`, and `ADDITIONS` is empty with the
comment "Empty at schema 1".

The board file on this machine proves it. `~/Library/Application Support/Quill/plugins/agent-tasks/board.sqlite3`
holds 26 columns on `task`, `owner` is not among them, and `meta.schema_version` is 1. `Store::claim`
writes `owner = ?5`, SQLite refuses the statement, and the message the operator saw is
`store.rs`'s own wrapping of that refusal. Every launch fails, which is why no ticket could be worked.

**The change.**

- `ADDITIONS` gains `("task", "owner", "TEXT")`, and `SCHEMA_VERSION` goes to 2, because a column was
  added and that is the only kind of change this schema makes.
- `check_version` stops being the only thing that writes the version. It refuses a file from a newer
  Quill as it does now, and then, **after** `migrate` has run, the version the file is at is written
  down as `SCHEMA_VERSION`. Today a file at version 1 stays at version 1 for ever however many columns
  are added to it, which makes the number meaningless.
- The order in `Store::prepare` becomes create, check, migrate, record.

**What proves it.** With no window: a board built with `owner` dropped by hand gains it when it is
opened again, and a ticket can then be claimed on it — the test is the exact failure, reproduced and
then fixed, rather than an assertion that the list has an entry in it. A second test opens a file
twice and asserts the version ends at 2 and no data is lost. The migration is additive and guarded by
`pragma_table_info`, so running it twice is a no-op, which the "opening it again changes nothing" test
already covers.

---

## 6. The Schedule view goes

> We don't need a Schedule field.

There is no field called Schedule on a ticket. What there is, is `View::Schedule`, the fifth of the
board's views, drawn by `lanes::schedule`, listing the cron rows in the `task_schedule` table with
when each next runs. Nothing on this board writes such a row: the browser board's scheduler is a
server that runs while nobody is looking, and `tasks/agent-tasks-plugin-tdd.md` lists it as absent.
So the view is a view of a table that is always empty.

**The change.** `View::ALL` becomes four, `View::Schedule` and `lanes::schedule` go, and the
`schedules` field on `AgentTasks` and its read in `refresh` go with them. `plugins run agent-tasks view
schedule` answers that there is no such view and names the four there are.

The `task_schedule` **table stays**. Dropping a table is deleting data, this schema has never dropped
anything, and a board file that has one is not harmed by having one.

**What proves it.** `View::parse("schedule")` is `None`, `View::ALL` has four entries, and the view
switch draws four buttons. The `store::schedules` read keeps its own tests, because the table and its
reader are still there.

---

## 7. The tickets that were cloned are cleared out

> Clear out existing tasks. They were cloned and are out of date.

The board on this machine holds seven tickets across three lanes, copied from the browser board when
the plugin was first written, and they describe work that is finished or that never applied here.

**The change.** A new command, `plugins run agent-tasks clear`, deletes every ticket on the board with
its todos and its comments, and answers with how many it deleted. It is a command rather than a
button: emptying a board is not something to put one press away from a person's hand, and the ticket
asks for it once.

Two safeguards, both because this is the one command here that destroys work:

- It **copies the board file first**, beside itself, named `board-before-clear-<timestamp>.sqlite3`,
  and the answer says where that copy is. Nothing is unrecoverable.
- It takes the word `confirm` as its argument. `plugins run agent-tasks clear` with nothing after it
  says what it would delete and deletes nothing.

Epics and sprints are left alone. They are not tickets, the active sprint is what the board draws
against, and a board with no sprint draws "No active sprint".

The operator's own board is cleared as part of this ticket, with the backup left in place, and the
comment on `task-28` says where it is.

**What proves it.** With no window: `clear` with no argument deletes nothing and says so; `clear
confirm` on a board of three tickets with todos and comments leaves no tickets, no todos and no
comments, and leaves the epics and the sprint; the copy exists and opens as a board holding the three
tickets. The cascade already deletes todos and comments with a ticket, so the test asserts the rows
are gone rather than trusting the foreign key.

---

## 8. The Settings page has a scroll bar that does not scroll

> The Quill -> Settings -> Plugins -> Agent-Tasks has a scroll bar but doesn't scroll.

`settings_page::show` takes `ui.available_rect_before_wrap()` **before** it opens the `ScrollArea`,
and hands that rectangle to `rows`, which paints every row at absolute coordinates measured from it.
Scrolling moves the inner `Ui`; nothing reads the inner `Ui`. So the bar moves and the page does not,
which is what was reported.

The bar appears at all because a few widgets are added with `ui.put`, which does allocate space in the
inner `Ui`, at coordinates far below its origin. That is also why the bar's length has no relation to
the page's length.

**The change.** `rows` reads its rectangle from the `Ui` it is given, inside the closure, so the
origin it lays out from moves with the scroll offset. At the end it allocates exactly the height it
drew, so the scroll bar describes the page. That is two lines and a parameter, and it makes the page
the first scrolling page in the Settings window, which `tasks/ui-plugin-architecture.md` §4.4 already
named as the moment to build one.

**What proves it.** Through the widget tree: the page is scrolled to the bottom and the last control —
`Clear`, beside the key — is present and reachable, which it is not today. The accepted screenshot is
retaken.

---

## 9. The ticket count leaves the Settings page

> I don't need tickets on this board in settings.

`Tickets on this board` runs `board.board().total()` and is a number the board's own header already
says, on a page that is about where the board is and how it connects.

**The change.** The row goes.

**What proves it.** The page has no control named `Tickets on this board`, and the screenshot is
retaken.

---

## 10. The connection settings become an Iliad URL and an Iliad key

> Prefill my settings with the base url for Iliad.
> Key name, key variable, authentication key: this is too confusing of a setup. We just want the
> minimum needed to connect. Should just be the iliad key from zshrc and illiad url. If base url is
> not provided it should just be default url for that model.

Today the page asks for four things to describe one connection: a `Base URL`, a `Key name` naming a
keychain entry, a `Key variable` naming an environment variable, and the key itself. Three of those
four are Quill's plumbing described to the person using it. What the connection actually is, is a URL
and a key.

Iliad is the gateway this machine already uses. `~/.zshrc` holds both values:
`ANTHROPIC_BASE_URL=https://iliad-emerging-api.abbvienet.com/api/llm`, and `ANTHROPIC_API_KEY`, with
`ILIAD_API_KEY` set to the same value for the Codex command line.

**The change.** Two settings under `Where the agent connects`, and nothing else.

- **Iliad URL.** The same `base-url` value in the settings file, prefilled with
  `https://iliad-emerging-api.abbvienet.com/api/llm` for a configuration that has never been written.
  Empty means the agent's own endpoint, which for Claude is `api.anthropic.com` and for Codex is
  OpenAI's — that is the ticket's "default url for that model", and it is what leaving both
  environment variables unset already does.
- **Iliad key.** One field. What is typed goes to the machine's keychain under a **fixed** entry name,
  `iliad`, so there is no name to choose and no variable to name. The row says `set` or `not set` and
  never the key, which is unchanged and is the rule that matters here.

`key_name` and `key_variable` leave `Configuration` and leave the settings file. `Configuration::environment`
reads the `iliad` entry and hands the value to the agent as all three names that matter:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and `ILIAD_API_KEY`. Setting a name the agent does not read
costs nothing; a name the agent needed and did not get is a board that cannot talk to the gateway.

**The key is taken from the environment when the keychain has none.** Quill launched from a terminal
inherits `ANTHROPIC_API_KEY` from `~/.zshrc`; Quill launched from the Dock does not. So
`Configuration::environment` reads the keychain first and falls back to this process's own
`ANTHROPIC_API_KEY`, and the Settings page says which of the two answered. That is what "the iliad key
from zshrc" means in practice, and it is why a person who launches Quill from a terminal has nothing
to type at all.

`ANTHROPIC_CUSTOM_HEADERS` is set too, to `x-api-key: <key>`, because `~/.zshrc` sets it and the
gateway is what needs it. It carries the key, so it is built at the moment of launch alongside the
other three and is never written anywhere.

A settings file holding the old `key-name` and `key-variable` lines is read without complaint and
those lines are dropped when it is next written, which is what `Configuration::read` already does with
a name it does not know.

**What proves it.** With no window: a configuration that has never been written has Iliad's URL;
`environment()` gives the four names when the keychain has a key; `environment()` falls back to the
process's `ANTHROPIC_API_KEY` when the keychain has none, and gives no key name at all when neither
has one; a settings file with `key-name` and `key-variable` in it reads and writes back without them.
And `SessionSettings` still prints variable names and never values, which the existing test covers.
Through the widget tree: the page has an `Iliad URL` and an `Iliad key` and no `Key name` or `Key
variable`; the key is saved and the row changes from `not set` to `set`.

---

## 11. Raw and rendered views on the description and on each comment

> The description, comments, etc should have icons to view as raw, or as markdown, if there is the
> markdown plugin installed (there is).

One correction to the premise, because the design depends on it: **there is no markdown plugin.**
`crates/quill-app/plugins/` holds agent-tasks, css, html, javascript, mermaid, rust and typescript.
Markdown is built into `quill-core` — `quill_core::markdown::render` is what the editor's own preview
is made of. So the toggle is always available and there is no plugin to check for. What the plugins
supply to a preview is syntax colouring inside a fenced code block, through `PluginHighlighter`, and
that is a decoration on the rendered view rather than a condition of it.

**The change.**

A new `components::markdown_text`, which renders a string of markdown into the same three things a
document holds and paints them with the painter the editor's preview already uses:
`quill_core::markdown::render` for the source, `quill_core::layout` for the lines, and
`components::editor_view::paint_text` for the glyphs. It needs a `TextRenderer` and a width, both of
which a plugin has: `plugin_ui::Look` carries the renderer, which is the same borrow that lets the
plugin draw a real terminal.

Two icon buttons, on the description's label row and on each comment's header, drawn with
`icon::code` for raw and `icon::eye` for rendered, the chosen one in its pressed state. They are the
same two buttons in both places and one function draws them.

- **The description** opens **raw**, because it is the field somebody writes in and a description that
  had to be switched into an editable state before it could be typed into would be worse than what is
  there now. Rendered is a read of it.
- **A comment** opens **rendered**, because a comment is read far more often than it is edited, and an
  agent's comments are markdown with headings, lists and code in them. `Edit` on a person's own
  comment still turns it into a raw field, which it already does.
- Which way each is showing is remembered on `Detail` for the ticket that is open, per comment id for
  the comments. It is not written to the database: it is how somebody is looking at a ticket right
  now, not a property of the ticket.

An agent reaches the same thing: `plugins run agent-tasks show description --rendered` and
`show comment <id> --rendered` set the same state through the same code, and the plugin's `view()`
reports which way each is showing, so an agent can read back what a person is looking at.

**What the rendered view does not have**, said here rather than discovered later: pictures and Mermaid
diagrams. `QuillApp::refresh_preview` resolves those in two extra passes that decode an image and lay
a diagram out, and both need the window rather than the plugin. A description with an image in it
shows the image's paragraph as its alt text. That is a limitation of this change and it goes in
`plugin.limitations`.

**What proves it.** With no window: `markdown_text` turns a heading, a list, a fenced code block and a
table into a layout with the right number of lines, which is `quill_core::markdown`'s own test shape.
Through the widget tree: the description's two buttons switch it and the rendered view shows a heading
larger than body text; a comment starts rendered and its raw button shows the source; `Edit` still
opens a raw field. Screenshots of both states.

---

## 12. The terminal, verified rather than assumed

> Verify that the terminal shows up and can be interacted with and looks like a normal claude code
> session.
> Verify that the terminal can be resumed regardless of agent done, etc.

These two are verification rather than change, and this section says what verification means, because
"it works" is what the ticket is asking to stop hearing.

A ticket's terminal is `components::terminal_panel::grid`, the same function the terminal tile and the
run tile draw, so it has Quill's keyboard, selection, mouse reports, resize and clipboard rules. That
is a claim about a shared function and the tests below are what make it evidence.

Resuming already does not depend on the lane. `AgentTasks::resume` reads the ticket, refuses only when
the ticket is assigned to a person, when the agent is Codex, or when a terminal is already **running**
for it, and it deliberately does not change the ticket's status — the comment on it says a ticket in
Agent Done that is resumed stays in Agent Done. What has never been tested is that the button is drawn
and reachable in that state.

**What proves it.**

- With no window: `resume` succeeds on a ticket in each of the four lanes, and on one whose terminal
  object exists but has exited, which is the case the code was specifically written for.
- Through the widget tree: a ticket's terminal takes typed characters and the characters reach the
  session; the grid is drawn with the terminal's own font and its own palette, asserted against
  `Look::monospace_size` and the terminal palette rather than by eye; `Resume session` is present on a
  ticket in `agent_done` whose session has exited, and absent while one is running.
- Driving a real agent: §13.

Codex is the one exception and it stays one. Codex names its own sessions, so a Codex ticket is started
again rather than resumed, and `why_it_cannot_resume` is the sentence a person reads. That is recorded
in `plugin.limitations` already.

---

## 13. Integration tests that drive a real agent

> We need agent task integration tests that actual interact with the model to ensure things work as
> expected.
> Set things up, add full integration testing, including interacting with the agent, making sure real
> time updates to the board occurs, etc.

Every test the plugin has today stops at the edge of the process. `agent::launch` is tested as a
command line, `Store` is tested in memory, and the widget tree is tested with no agent behind it. So
the one thing nobody has watched is the thing the plugin is for: an agent starting, reading its
handoff, writing to the board, and the board showing it while it happens.

This is the `tools/agent-study` bargain applied to the board: drive the real thing and grade what
happened against Quill's own state read back, rather than against what the agent said it did.

**Where it lives.** `crates/quill-app/tests/agent_board.rs`, a test file of its own, because it starts
processes and takes minutes while `screenshots.rs` takes seconds. Every test in it is
`#[ignore]`d, so `cargo test` stays fast and `cargo test --test agent_board -- --ignored` is what runs
them. A test that cannot find the `claude` program on the path skips with a message naming what was
missing rather than failing, which is what `debuggers.rs`'s own tests do about `lldb-dap`.

**What each one drives.**

1. **A ticket is claimed and worked.** Create a board in a temporary folder, create a ticket whose
   description asks for one small provable thing — write a named file into the temporary project
   folder with a given line in it — and start the agent. Then wait, with a ceiling, on Quill's own
   state: the ticket's status is `in_progress`, `agent_session_id` and `owner` are set, and the file
   exists with that line. This test is what would have caught §5, because claiming is the first thing
   it does.
2. **The board updates while the agent works.** The same run, sampled: the terminal's scrollback grows,
   and each sample is taken through `AgentTasks::tick` the way the window pumps it, so what is asserted
   is the path the window uses rather than a read of the process.
3. **A comment reaches the agent and the agent answers on the board.** Post a comment asking for a
   named reply, and wait for a comment from the agent whose body holds it. That is the whole loop the
   protocol depends on: board to terminal to agent to board.
4. **A retired session is resumed and remembers.** Tell the agent a token, stop the terminal, resume
   the ticket, ask for the token back, and read the answer off the board. This is what proves resume is
   the conversation rather than the process, and it runs on a ticket in `agent_done`, which is §12's
   claim.
5. **The watchdog nudges a quiet agent and stops when it hears back.** With the lease set to a minute,
   `tick` records a strike and posts its nudge, and a heartbeat clears it.

**What the model is.** The tests read `ANTHROPIC_BASE_URL` and `ANTHROPIC_API_KEY` from the environment,
which is Iliad on this machine, and pass `--model` from an environment variable with a default. They
are `#[ignore]`d partly for time and partly because they cost tokens, and a test that spends money
should be one somebody asked for.

**What they do not do.** They do not assert on the agent's wording. Every assertion is a state Quill
holds or a file on disk, because grading a model's prose is a test that fails when the model improves.

---

## 14. The order this is built in

The migration in §5 comes first, because nothing on the board can be started until it is done and §13
cannot run at all. Then the explorer in §1, because it is the fault that makes the window unusable.
Then the manifest change in §3, since it decides which components the rest of the work touches. Then
§2, §4, §6, §8, §9, §10 and §11 in any order. Then §7 against the operator's own board. Then §12 and
§13, which are the tests that need everything else to be true.

## 15. What is deliberately not in this

- **A background thread for reading a folder.** §1 makes the read cheap enough that it does not need
  one. A thread would be the right answer for a folder on a network drive, and it is a larger change
  than this ticket asks for: the tree would gain a pending state, every row that draws would have to
  answer for it, and the explorer's tests would all need a wait.
- **Pictures and Mermaid diagrams in a rendered description.** §11 says why and what it would take.
- **A Windows keychain.** Unchanged from `tasks/agent-tasks-ui-tdd.md` §10. The Iliad key field is
  absent on Windows and the page says so.
- **Dropping the `task_schedule` table.** §6. Nothing in this schema has ever dropped anything.
- **Deleting `agent_tasks_pane.png`.** §3. It is recommended to the operator instead.

---

## 16. What building this found that the ticket did not ask about

Three things turned up while the work was done. Each is recorded here because a reader of this document
a month from now will want to know why the code says what it says.

### 16.1 The watchdog could never give work back

`store::add_comment` called `touch`, and `touch` sets `heartbeat_at`, `watchdog_strikes = 0`,
`watchdog_nudges = 0` and `watchdog_nudged_at = NULL`. The watchdog's own strike posts a `system`
comment saying it struck — so the strike cleared itself on the way out. `watchdog::decide` reaches
`Decision::Reclaim` when `strikes + 1 >= strikes_before_reclaim`, and with `strikes` stuck at zero it
never did. A ticket whose worker had gone was struck for ever and never reclaimed, which is the one
thing the watchdog exists to do.

The integration test in §13 is what found it: the pure tests over `watchdog::decide` were all correct,
because `decide` was correct. Nothing had ever checked that a decision was written down.

**The fix.** A comment from a person or an agent is somebody saying something and counts as activity. A
comment whose author is `system` is the watchdog talking to itself and does not. That is one condition
in `add_comment`, and it has a test named after the fault.

There is a second, milder version of the same thing left alone deliberately: a strike still writes
nothing to `heartbeat_at` now, so the escalation runs on strikes as it was designed to, but a **person's**
comment on a stalled ticket clears its strikes. That is correct — somebody is watching it.

### 16.4 An agent started in a folder nobody has opened before stops at a trust question

The first of the agent driven tests recorded a claim, a session and an owner on the ticket, and then the
agent exited having written nothing. The test said only that the file was missing, so the test was given
two things it should have had: it stops waiting when the agent is no longer running, and it prints the
last screenful of every terminal when it fails. The screen said:

```
 Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open
 source project, or work from your team). If not, take a moment to review what's in this folder first.

 ❯ No, exit
   Yes, I trust this folder
```

Claude Code asks this the first time it is started in a folder it has not seen, and
`--dangerously-skip-permissions` does not answer it. So **a ticket whose project is a folder nobody has
opened before starts an agent that never begins**, and the board is left holding a claim, a session and an
owner for work nothing is doing — which looks exactly like an agent that is thinking.

In practice a ticket usually names a folder somebody already works in, which is why this was not noticed
before: the question is asked once per folder, ever. It is still the second reason an agent could not be
made to do work, after §5.

**What the tests do about it.** They set `CLAUDE_CONFIG_DIR` to a folder of their own and record the
answer there, so nothing is written to the operator's `~/.claude.json` and the trust is recorded for a
temporary folder rather than for anything of theirs. Both the path and its canonical form are recorded,
because a folder under `/var` on macOS is reached through a symlink to `/private/var`.

**What the plugin does about it: nothing, yet, and that is deliberate.** Answering the question for
somebody is a decision about their machine, not about this board, and the two ways to do it are both worse
than asking the operator. Typing keystrokes at a prompt is exactly the fragile prompt reading
`services::agent_tasks::agent` already refuses to do. Writing `hasTrustDialogAccepted` into
`~/.claude.json` on their behalf is Quill silently granting an agent the run of a folder. What the board
**should** do is notice it and say so — the terminal is right there and the words are on it — and that is
a ticket of its own rather than a change smuggled into this one.

### 16.5 An agent started from inside another Claude Code session does not get its own conversation

The agent driven tests reach their claim assertions and fail after them, and the screen the test now prints
says why: `Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker`, followed by the parent
session's transcript rather than the ticket's handoff. `--session-id` names a fresh conversation and the
child session marker overrides it.

That is about **where the tests were run from**, not about the board: these were run from inside a Claude
Code session, so the agent Quill spawned was that session's grandchild. A Quill started from the Dock or a
plain terminal has no such marker.

So the claim half of §13 is verified against a real agent — the ticket moves to `in_progress`, the session
id and the `owner` are written, and the command line is
`claude --dangerously-skip-permissions --model <model> --session-id <uuid>` — and the work half has to be
run from a plain terminal:

```
QUILL_TEST_PROJECT=<a folder you have already opened Claude Code in> \
  cargo test -p quill-app --test agent_board -- --ignored --test-threads=1
```

The test says this itself when it sees the marker, rather than leaving somebody to work it out.

### 16.2 The screenshot tests cannot run from a terminal that is not in a desktop session

Every test in `crates/quill-app/tests/screenshots.rs` fails in this session with
`Failed to create render state: RequestDeviceError(RequestDeviceError { inner: Core(Device(Lost)) })`,
including tests this work never touched. The machine's graphics card is fine and Quill itself is running
on it. The cause is that the shell the tests were run from is not in a window server session, so `wgpu`
cannot be given a device. The metal, vulkan and gl backends all behave the same, and so does running
outside the sandbox.

So the widget tree and screenshot tests written for this ticket are written and have not been run. They
have to be run from a terminal in a desktop session:

```
cargo test -p quill-app --test screenshots
```

Everything that does not need a graphics device — 766 library tests and the integration tests in §13 —
has been run.

### 16.3 A queued command does not wake a window that has stopped drawing

An attempt to run the screenshot tests inside the running Quill's own terminal, through the MCP control
channel, found that Quill did not answer at all: `terminal list` timed out with
`It has not drawn a frame for 331.9 s and 2 requests are queued, so it is not drawing rather than busy`.

A control request that arrives while the window is idle should wake it. This is not on `task-28` and was
not chased; it is recorded so it can be a ticket of its own.

## 17. Where the implementation differs from what §1 to §15 planned

- **§1.1 keeps the size refusal.** The plan said the length comes from the directory read on every
  platform. It comes from one `DirEntry::metadata` call, which is one syscall per child — the same
  number the old code spent on `std::fs::metadata` inside `openable`, and one fewer than it spent in
  total, because the `open` and `read` are gone.
- **§4's dropdown lives in one place, not in `ticket_modal`.** `components::agent_tasks::value_dropdown`
  draws the list, and the ticket and the Settings page each wrap it with their own label furniture. The
  plan implied one wrapper; two labels with two different weights is what the two pages already had.
- **§4 needed one thing the plan did not mention.** The text above each dropdown used to register a
  `Label` in the accessibility tree carrying the same name as the control, so a test asking for `Model`
  could not tell which node it had. The words are painted now and the control answers to the name.
- **§13's watchdog test does not drive an agent.** `watchdog::decide` is pure and already tested for
  every decision, so what the integration test adds is that a decision is **written down**, which needs
  no agent: a claimed ticket this window has no terminal for is a ticket whose worker is gone. That is
  also what found §16.1.
