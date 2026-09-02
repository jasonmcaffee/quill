# task-1777 — the Database plugin

> We want a plugin with ui that allows us to see dbs, write queries, update rows, etc similar to
> IntelliJ db ui.
>
> Provide Postgres and sqllite connectivity for our first iteration.
> Do online research, collect screenshots, for how IntelliJ works, then write a TDD. We want an
> amazing UX and ui. Use the same ui design that our agent tasks used with crate vello.
>
> Once done, fully implement.
>
> Verify things work with test Postgres and sqllite dbs.
>
> Have another agent review your pr, and address any concerns it has.
>
> There is another agent working on a themes plugin, so find a way to do your changes in parallel
> without affecting their work.

A pane, a tab, a settings page and a crate. `crates/quill-db` speaks to a database and knows nothing
about a window; `services::database` is the provider named by `plugins/database/plugin.conf`; and
`components::database` draws it in the dark neumorphic chrome `task-1765` built for the Agent-Tasks
board. The research this is measured against is in
`_agent_output/task-1777-database-plugin/intellij-research.md`, with the screenshots beside it.

## 1. What IntelliJ actually is, and which third of it is worth copying

Three surfaces that refer to each other, not one window. Getting that split right is most of the
design, and it maps onto contributions Quill's plugin manifest already offers.

| IntelliJ | What it is | Quill |
|---|---|---|
| **Database tool window**, docked right | The tree: data sources → databases → schemas → tables → columns, with counts on the folders and a `2 of 5` badge that opens a schema chooser | the plugin's **pane** |
| **Query console**, a tab | A SQL editor attached to one data source and one default schema; Execute runs *the statement under the caret*; `Output` and `Result 1` tabs under it | the plugin's **tab** |
| **Data editor**, a tab | One table's rows: a `WHERE` and an `ORDER BY` field of typed SQL, a row-number gutter, per-column sort, paging, and edits that accumulate as pending changes with a preview before Submit | the same **tab**, a second kind of inner page |

`_agent_output/task-1777-database-plugin/intellij/db_database_tool_window.png`,
`db_query_console_overview.png` and `data_editor_db_object_data.png` are those three, downloaded from
JetBrains rather than remembered.

**What is worth copying, and it is a short list.** The tree with counts and a schema chooser. `F4` to
open a table's rows. Execute-the-statement-under-the-caret, because a console holds several and
running the file is never what was meant. `WHERE` and `ORDER BY` as *typed SQL* rather than a filter
builder — it is the thing everybody who uses that grid actually types in. Paging that says `1-200 of
201+`, because a count that claims to be exact when the server was never asked for one is a lie.
Pending changes previewed as the statements they will become, before anything is written. NULL and
DEFAULT as explicit values a cell can be set to rather than words somebody types. And a Test
Connection that answers with the **server's own version string**, which is `quill-git`'s rule that a
server's own words are what a report quotes.

**What is deliberately not copied** is §11, and it is a much longer list, because most of the
Database Tools plugin is breadth across fifteen engines rather than depth in two.

## 2. What was weighed: how Quill talks to PostgreSQL

This is the decision the whole feature rests on, and the obvious answer is the wrong one.

| | What it is | Verdict |
|---|---|---|
| **`postgres` 0.19** | The blocking wrapper over `tokio-postgres` | **Refused.** It is a blocking *façade* over an async client: it builds a Tokio runtime and runs the connection on it. Quill has no runtime, on purpose — `quill_git::Worker`, the terminal's reader, `text_search`, `symbol_index`, `quill-dap` and `quill-chat` are all plain threads with channels, and the `ureq` decision in the workspace `Cargo.toml` says in as many words that "a runtime added for one pane would be a second concurrency model in a program that has one". It also costs on the order of fifty crates. |
| **Shell out to `psql`** | `quill-git`'s answer, applied to a database | **Refused, and it is worth saying why**, because the precedent looks strong. `git` is on this machine because the person uses git; `psql` is not necessarily on a machine that has a Postgres *server* somewhere else, and the whole point of a data source is that the server is elsewhere. And `psql`'s output is a report meant for a person: a value containing a newline, a NULL against an empty string, and a column whose name contains the separator are all ambiguous in it. `git`'s porcelain formats were designed to be parsed; `psql`'s were not. |
| **A JDBC-shaped driver layer** | What IntelliJ has: one interface, a driver per engine, downloaded on demand | **Refused.** Nothing in Quill is ever fetched, which is a rule with no exceptions, and a driver interface with two implementations is an abstraction invented before the second case has been seen. Two engines get two modules and one `enum`. |
| **The wire protocol, written here** | `crates/quill-db`, speaking PostgreSQL v3 over a `TcpStream` | **Chosen.** It is exactly what `quill-dap` does for the Debug Adapter Protocol and what `quill-chat` does for server-sent events, down to the test strategy: a **scripted server** replaying fixed bytes on a `TcpListener` bound to `127.0.0.1:0`. The protocol is small, stable since 2003, and the parts a database explorer needs are a fifth of it. |

### 2.1 What that costs in crates, counted rather than guessed

The workspace already carries `sha2` 0.10 and `digest` 0.10 (through `wry`), `base64` 0.22 (through
`alacritty_terminal`), `native-tls` (through `ureq`) and `getrandom`. So the whole of the PostgreSQL
client adds **two crates to the tree**: `hmac` and `md-5`, each of which is a thin layer on the
`digest` traits already here.

Two, against roughly fifty for `tokio-postgres`. That is the same shape of measurement the `ureq`
choice records — "31 crates in the tree against 121 for rustls with its own root store" — and it is
the argument, not decoration on one.

PBKDF2 is written here rather than taken as a crate: it is a loop of HMACs, twelve lines, and the
`pbkdf2` crate's own surface is the `password-hash` framework this does not want. Base64 is the
crate, because it is already in the tree and hand-written base64 is a place to put an off-by-one.

### 2.2 SQLite needs nothing new

`rusqlite` with SQLite's own C compiled in is already a workspace dependency, for the Agent-Tasks
board. A SQLite data source is a file path; there is no server, no port and no password, and
`sqlite_master` plus `PRAGMA table_info` is the whole of introspection.

**And SQLite is what the tests are built on.** A screenshot test cannot depend on a PostgreSQL server
being up on the machine running it, so every test that needs a real database builds a `.db` file in a
temporary folder. The PostgreSQL half is tested against the scripted server and, separately, by hand
against the real 17.2 on this machine — §10 says which is which.

## 3. `crates/quill-db` — the half with no window in it

The sixth crate, arranged as `quill-dap` and `quill-chat` are: the wire, the values, the session, and
the thread. It depends on `rusqlite`, `sha2`, `hmac`, `md-5`, `base64`, `getrandom` and `native-tls`,
and on nothing that draws.

```
quill-db
  source.rs      what a data source is, and reading one out of a URL
  value.rs       a cell: null, an integer, a float, text, bytes — and what a column's type is called
  rows.rs        a result: the columns, the rows, how long it took, how many were affected
  postgres/
    wire.rs      the frames: a tag, a length and a body, in both directions
    startup.rs   the startup message, the parameters, BackendKeyData, ReadyForQuery
    scram.rs     SCRAM-SHA-256, and the two older authentication messages
    session.rs   the connection: simple query, extended query, cancel, terminate
    introspect.rs the catalogue queries: databases, schemas, tables, columns, keys
  sqlite/
    session.rs   the same four things over `rusqlite`
    introspect.rs `sqlite_master` and `PRAGMA table_info`
  engine.rs      the enum both sit behind, and the one trait-shaped surface the window sees
  worker.rs      the thread a query runs on, and the channel the answer comes back down
```

### 3.1 A value is text, and that is a decision

Every value comes back **as the text the server printed**, plus the column's type name and a flag
saying whether it is a number. PostgreSQL's `RowDescription` names a type by OID and the protocol will
send either text or binary; asking for text means one decoder rather than one per OID, and it means
the grid shows exactly what `psql` would show for a `numeric`, a `timestamptz`, a `jsonb` or an array
without Quill deciding how to render any of them.

What the type is still used for, because the text alone is not enough:

- **Alignment.** A number is right-aligned in the grid and everything else is left-aligned, which is
  the one piece of formatting the type has to decide.
- **NULL against the empty string.** They are different and a grid that shows both as nothing is a
  grid nobody can trust, so NULL is drawn as a dim `NULL` in italics and never as text.
- **Binary.** `bytea` and SQLite blobs are shown as a size and the first bytes in hex, not as mojibake.
- **Quoting on the way back.** A pending change binds a *parameter*, so nothing is quoted by hand at
  all — §6.

### 3.2 The PostgreSQL frames that are implemented

The client sends: `StartupMessage`, `SSLRequest`, `PasswordMessage`, `SASLInitialResponse`,
`SASLResponse`, `Query`, `Parse`, `Bind`, `Describe`, `Execute`, `Sync`, `CancelRequest`, `Terminate`.

The server's frames that are read: `AuthenticationOk`, `AuthenticationCleartextPassword`,
`AuthenticationMD5Password`, `AuthenticationSASL`/`Continue`/`Final`, `ParameterStatus`,
`BackendKeyData`, `ReadyForQuery`, `RowDescription`, `DataRow`, `CommandComplete`,
`EmptyQueryResponse`, `NoData`, `ParseComplete`, `BindComplete`, `PortalSuspended`, `ErrorResponse`,
`NoticeResponse`, `NotificationResponse`, `ParameterDescription`.

Anything else is skipped by its length rather than treated as a fault, which is what the protocol asks
of a client and what keeps a future server version from breaking this one.

**The framing is tested by being fed the same stream split at every byte boundary**, which is the test
`quill-chat`'s server-sent-event reader already has. A frame that arrives in three `read` calls has to
produce the same message as one that arrives in one.

### 3.3 Authentication, and why SCRAM is not optional

Measured on this machine: **PostgreSQL 17.2, `password_encryption = scram-sha-256`.** An
implementation with only MD5 in it would not connect to Jason's own database, so SCRAM-SHA-256 is the
first thing written, not the last.

It is RFC 5802 with RFC 7677's hash, and the shape is:

```
client-first-bare  n=,r=<24 random bytes, base64>
server-first       r=<client nonce + server nonce>,s=<salt>,i=<iterations>
SaltedPassword     PBKDF2-HMAC-SHA-256(password, salt, i, 32)
ClientKey          HMAC(SaltedPassword, "Client Key")
StoredKey          SHA-256(ClientKey)
AuthMessage        client-first-bare + "," + server-first + "," + "c=biws,r=<nonce>"
ClientProof        ClientKey XOR HMAC(StoredKey, AuthMessage)
client-final       c=biws,r=<nonce>,p=<proof>
ServerSignature    HMAC(HMAC(SaltedPassword, "Server Key"), AuthMessage)
```

Three things about it are decisions rather than transcription.

**The server's signature is verified.** `AuthenticationSASLFinal` carries `v=`, and a client that
ignores it has thrown away the half of SCRAM that proves the *server* knew the password. It is checked
in constant time and a mismatch is a refusal naming the server, not a warning.

**The nonce is from the operating system**, through `getrandom`, and never from a counter or the
clock. A predictable client nonce weakens exactly the replay property the exchange exists for.

**SASLprep is not implemented, and the limitation is written down rather than hidden.** Normalising a
password needs Unicode NFKC and a stringprep profile, which is a table-driven dependency for a case
that does not arise on this machine. An all-ASCII password — every password in the RFC's own examples
and every one this will meet here — is unchanged by SASLprep, so the raw UTF-8 bytes are used and
`plugin.limitations` says a password containing characters SASLprep would fold may be refused by the
server. That is the same shape as the Windows-keychain sentence in `services::agent_tasks::keychain`:
say the gap plainly rather than let it be discovered.

`AuthenticationCleartextPassword` and `AuthenticationMD5Password` are implemented too, because a
server on this network may still be configured for either; cleartext is refused unless the connection
is TLS or the host is loopback, which is a rule the client can enforce and should.

### 3.4 TLS is the machine's own, which is the `ureq` argument again

`sslmode` takes `disable`, `prefer` (the default) and `require`. `prefer` sends `SSLRequest` and
carries on in the clear if the server answers `N`; `require` refuses. The stream is wrapped with
`native-tls`, which is schannel on Windows and Security.framework on macOS — so the certificates a
data source is checked against are the certificates the machine trusts, exactly as for `ureq`, and
the crate is already in the tree.

`verify-full` is not offered, and nor is a certificate file: `require` means encrypted with the
platform's own verification. A data source that needs a private CA is a data source whose CA belongs
in the machine's store, which is where every other program on it would look too.

### 3.5 A query runs on a thread, and stopping it sends a second connection

One worker thread per connected data source, holding the connection, reading a channel of jobs and
answering down another — `quill_git::Worker` with a different payload. The window never blocks; it is
handed a `Ticket` and asks whether it has finished, and `Context::wake` is what brings the frame back
when it has.

**Stop is a real cancellation, not a flag.** PostgreSQL cancels by opening a *second* connection and
sending `CancelRequest` with the process id and secret key from `BackendKeyData` — a flag on this side
would leave the server working for as long as the query takes and the pane pretending otherwise.
SQLite has `sqlite3_interrupt`, which `rusqlite` exposes as an interrupt handle that can be held by
another thread. Both are wired to the same stop button, which is what the toolbar in
`data_editor_db_object_data.png` has.

## 4. The plugin, as data

`crates/quill-app/plugins/database/plugin.conf`:

```
plugin.id          = database
plugin.name        = Database
plugin.kind        = ui
plugin.description = …
plugin.limitations = …

ui.provider = database
ui.chrome   = vello

pane.id      = explorer
pane.label   = Database
pane.icon    = database
pane.side    = right
pane.group   = top
pane.width   = 340
pane.height  = 320
pane.applies = always

tab.id    = workspace
tab.label = Database
tab.icon  = database

menu.name    = Database
menu.entries = open-pane=Show Databases, open-tab=Open Workspace, -, new-source=New Data Source, refresh=Refresh, -, run=Execute Statement, submit=Submit Changes

settings.page = Database
settings.icon = database
```

Every line of that is arrangement a person can change by hand, and none of it can name a colour,
which is the rule `tasks/ui-plugin-architecture.md` §2.4 sets and this plugin keeps.

`pane.side = right` because that is where IntelliJ docks it and the ticket asks for the IntelliJ
shape; it is one word and `task-1697`'s docking moves it anywhere with no code here.

**Two names are added to registries**: `database` to `plugins::UI_PROVIDERS` and to
`plugin_ui::provider`, and `database` and `table` to `plugins::PANE_ICONS` with drawings in
`theme::icon`. A cylinder and a grid, drawn rather than lettered, so both take the tint the rail gives
them — `design/style-guide.md`'s rule.

### 4.1 A `sql` language plugin ships with it

A console with uncoloured SQL in it would be the one text field in Quill that looks like Notepad, and
Quill already has the machinery: a `language` plugin with keywords, a comment, a string and a hex
rule. `plugins/sql/plugin.conf` is that, and it is worth having on its own — a `.sql` file in a
project is coloured whether or not anybody opens the database pane.

The console reads it through `Look::highlighter`, which is the same `CodeHighlighter` the Markdown
preview colours a fenced block with, so the console and a `.sql` file agree by construction rather
than by two lists being kept in step.

## 5. The pane: the tree

`components::database::tree`. A header, a toolbar of five icon buttons, a filter field, and rows.

The toolbar, which is IntelliJ's eight cut to the five that apply here: **New data source**,
**Refresh**, **Disconnect**, **Open console** and **Edit data**. A control that cannot apply is
absent, so `Disconnect` is not there while nothing is connected and `Edit data` is not there unless a
table is chosen.

A row is 28 points, which is `size::ROW` and what every list in Quill uses. The tree is:

```
▾ ai                       PostgreSQL · localhost:5432
  ▾ public                 (schema)
    ▾ tables  27
        conversation
        member
    ▸ views  8
    ▸ routines 3
  ▸ information_schema
▸ tasks.db                 SQLite · C:\jason\dev\quill\tasks.db
```

Counts on the folders are IntelliJ's and they earn their place: they are how you tell an empty schema
from one that has not been introspected yet. Expanding a table lists its columns with a key mark on
the primary key and a dim type name after each, which is the one place the type is worth showing.

**Introspection is lazy and one level at a time.** Opening a data source lists its schemas; opening a
schema lists its tables; opening a table lists its columns. A database with four thousand tables in it
is why: IntelliJ's own answer to that is an introspection-level setting, and lazy loading is the same
answer without a setting. Each level is one catalogue query, run on the worker thread like any other,
so a slow server makes a row show a spinner rather than making the window stop.

The filter field narrows the loaded rows by substring, which is IntelliJ's speed search with a field
instead of type-ahead, because Quill's panes do not have type-ahead anywhere and inventing it in a
plugin would be the plugin deciding what a Quill pane is.

## 6. The tab: consoles and row editors

`components::database::workspace`. The plugin contributes **one** tab, so that tab holds a strip of
its own pages — the shape the Services tool window has in
`db_ui_query_console_result_tab.png`, and the honest answer to a manifest that offers one `tab.id`.

Two kinds of page:

### 6.1 A console

A toolbar — **Execute** (`Ctrl+Enter`), **Stop**, **History**, the row limit, and, pinned right, the
**schema switcher** reading `ai.public` — over a SQL editor, over the results.

**Execute runs the statement under the caret**, which is IntelliJ's behaviour and the only one that
makes a console holding six statements usable. The statement boundaries come from a small splitter
that knows about `;`, string literals, dollar-quoted bodies and `--`/`/* */` comments — the same
awkward cases `quill_core::syntax` already has to know for colouring, but written once here because
the boundary question is not the colouring question. `Ctrl+Shift+Enter` runs everything, in order,
stopping at the first failure and saying which statement failed.

The editor is an `egui::TextEdit` with a layouter that colours through the highlighter — **not** a
second copy of Quill's editor**.** It has selection, undo and the clipboard, and it does not have
folding, multiple carets, the gutter or find-in-file. That is written in `plugin.limitations` rather
than left to be discovered, and the reason is the one `tasks/ui-plugin-architecture.md` gives: a
provider draws inside the rectangle it is handed and cannot reach `components::editor_view`.

What it does have that a plain field does not is **completion of names it already knows**: `Ctrl+Space`,
and automatically after a `.`, offers the tables of the current schema and the columns of the tables
named in the statement being written. The candidates come from the tree's own introspection, so
nothing is fetched to offer them and there is no second index — which is `task-1677`'s rule that the
candidates are the ones something already keeps.

Results appear underneath as tabs: `Result 1` for a statement that returned rows, `Output` for one
that did not, carrying the affected count and the elapsed milliseconds. A statement that fails puts
the server's own message in `Output`, verbatim, with its `SQLSTATE`, its detail and its hint — the
rule `quill-git` keeps about quoting a program's own words.

### 6.2 A row editor

Opened by `Edit data` on a table, or by double-clicking it in the tree, or by `plugins run database
open <table>`.

A toolbar of exactly the buttons in `data_editor_db_object_data.png` that apply: **Reload**,
**Add row**, **Delete row**, **Revert**, **Preview pending changes**, **Submit**, and the page
controls. Under it two fields, **`WHERE`** and **`ORDER BY`**, each a fragment of SQL. Under those the
grid: a row-number gutter, a header per column with the type and a sort chevron, and the rows.

Paging is `LIMIT n OFFSET m` with `n + 1` asked for, so `1-200 of 200+` is honest about not having
counted — which is what IntelliJ's own `of 501+` means.

Sorting sends a new `ORDER BY` rather than sorting the page, because sorting the page you fetched is
a different answer to the question and the grid cannot say which one it is showing.

### 6.3 Editing, and the one thing that makes it safe

**A row can only be changed if it can be addressed.** A result is editable when it came from one table
and that table has a primary key — or, in SQLite, a `rowid`. Otherwise the grid is read-only and the
Add, Delete and Submit buttons are **absent**, with one line saying why: *"these rows have no key, so
there is no way to change one without changing others."* That is the absent-control rule doing real
work: the alternative is an `UPDATE` matching on every column, which silently updates two identical
rows.

A console result is never editable. IntelliJ tries to resolve one back to a table; Quill does not
promise what it cannot enforce, and the console has a `WHERE`-and-`ORDER BY` grid one click away in
the row editor.

**A change is pending until it is submitted.** Editing a cell records `Set { key, column, value }`;
Add and Delete record their own. The grid shows a pending cell in the `modified` colour the git panel
already uses and a pending row with the `added` or a struck-through mark, so what is written and what
is not is visible without pressing anything.

**Preview shows the statements.** Not a summary of them — the actual `UPDATE … WHERE <pk> = $1`,
`INSERT INTO … VALUES ($1, $2)` and `DELETE FROM … WHERE <pk> = $1`, with the parameters listed
beside them. It is a modal built from `components::modal`, like every other modal in Quill.

**Submit is one transaction.** `BEGIN`, the statements in order with their values bound as
**parameters** — nothing is quoted by hand anywhere, which is what makes a value containing a quote, a
newline or a backslash a non-event — then `COMMIT`, or `ROLLBACK` and the server's own message if any
statement fails. The affected count of each is checked: an `UPDATE` that reports zero rows means the
row moved underneath, and that rolls the whole thing back and says so rather than reporting success.

There is **no manual transaction mode**, and that is a refusal with a reason: an editor that can leave
a transaction open is an editor that can hold locks on somebody's database while nobody is looking at
the window, and the failure is invisible until something else blocks. Every submit is its own
transaction.

## 7. Where a data source lives, and where its password does not

Data sources are written to `<settings folder>/plugins/database/sources.conf`, in the same
`services::store::Values` format the settings file and every plugin manifest use.

```
sources = 2

source.0.name     = ai
source.0.engine   = postgres
source.0.host     = localhost
source.0.port     = 5432
source.0.database = ai
source.0.user     = postgres
source.0.sslmode  = prefer
source.0.password.env = QUILL_DB_AI
source.0.read_only    = false

source.1.name   = tasks
source.1.engine = sqlite
source.1.file   = C:\jason\dev\quill\tasks.db
```

**No password is ever written by Quill**, which is `services::agent_tasks::keychain`'s rule and
Agent-Chat's. Three ways a data source gets one, and the settings page says `set` or `not set` and
never the value:

- `password.env` names an **environment variable**, read at the moment a connection is opened and
  never held. It is the only route on Windows, which has no keychain here.
- `password.keychain` names an entry in the machine's own keychain, read through the existing
  `keychain` module — macOS and Linux only, and the page says so there rather than offering a control
  that cannot apply.
- Typed into the connect dialog, in which case it lives **in this process and nowhere else** and is
  gone when the window closes. Where IntelliJ's dialog offers `Save: Forever`, Quill's says
  `until this window closes`, and that is the whole of the choice.

A refusal names the variable or the entry — never the value, and a server's message is scrubbed of it
before being quoted, which is the redaction rule `quill-chat` already keeps for an API key.

**`read_only` is enforced by the server, not by a parser.** On connect, a read-only PostgreSQL data
source runs `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` and SQLite is opened with
`SQLITE_OPEN_READONLY`. Quill also refuses to *offer* the editing controls, but the guarantee is the
server's — the same distinction Agent-Chat's settings page draws between Codex's sandbox and Claude's
permission mode, and for the same reason: a promise the window cannot keep should not be made.

## 8. The agent's half

Everything above is reachable through `plugins run database <command>` and readable through
`plugins view database`, which are catalogue commands that already exist — so the MCP tools are
generated for it the day it ships, with no new rows and no hand-written tool. That is the machinery
`CLAUDE.md` describes working as intended.

| Command | What it does |
|---|---|
| `open-pane`, `open-tab` | Put the tree, or the workspace, on the screen |
| `sources` | Every data source, its engine, where it points, whether it is connected, whether it has a key |
| `add-source <name> <url> [VARIABLE]` | `postgres://user@host:port/db?sslmode=require`, or a path for SQLite, and optionally the **name** of an environment variable holding the password |
| `password <name> env <VARIABLE>` | Where the password is: `env`, `keychain` or `none`. Never the password |
| `remove-source <name>` | |
| `connect <name>`, `disconnect <name>` | |
| `use <name>` | Which data source the console and the tree are pointed at |
| `schemas`, `tables [schema]`, `columns <table>` | The tree as data, introspected on demand |
| `ddl <table>` | The `CREATE TABLE` statement, which is IntelliJ's `Go to DDL` |
| `open <table>` | Open the row editor on it |
| `query <sql>` | Start a query. **Does not wait** |
| `state [id]` | Running, finished or failed, with the elapsed time |
| `result [id]` | The columns and rows, bounded by the row limit |
| `set <row> <column> <value>`, `add-row`, `delete-row <row>` | Record a pending change |
| `pending` | The pending changes, with the statements they will become |
| `submit`, `revert` | Write them, or throw them away |
| `read-only <name> on\|off` | The switch the server enforces |
| `confirm on\|off` | Whether a console statement that changes rows is confirmed first |
| `view` | Everything the pane and the tab are showing |

**`query` does not wait, and that is the same decision Agent-Chat's `send` records**:
`UiProvider::command` runs inside a frame, and a command that blocked would stop the window drawing
for the length of a query. `state` says when it has finished, which is the shape `run start` and
`run output` already have. The summary of `query` says so, because an agent that does not know will
ask for the result too early exactly once.

**A destructive statement from an agent is still just a statement**, and the read-only switch is what
stands in front of it. It is written down in `plugin.limitations`: a data source that is not read-only
can be changed by anything that can reach the plugin, which includes a model given Quill's tools.

## 9. Drawing it: the same chrome as the board

`ui.chrome = vello`, so the pane and the tab record `Decor` into the canvas
`services::vello_canvas` rasterises, exactly as the Agent-Tasks board does. The recipes are already
there and no component holds a shadow offset:

- The pane's ground is `board_page`; the toolbar and the filter field are `sunken` wells; a chosen
  tree row is the `selected_row` pill every list in Quill draws.
- A console and a row editor sit on a `raised` card of `board_card` over the tab's `board_page`, with
  the results grid in a `sunken` well — which is the lane-and-card ladder the board already uses, one
  level shallower.
- The Execute button is the board's diagonal gradient with its glow, because it is the one button on
  the page that does the thing the page is for, and that is what the board's own primary buttons are.

Two costs are watched, and both have a measured precedent:

- **A hover does not change the decoration.** Moving the pointer across a grid of two hundred rows is
  the commonest thing anybody does in it, and re-rasterising for it would be the whole cost, all the
  time. The hover is a wash `egui` paints on top — `task-1765`'s rule, kept.
- **Only rows that intersect the clip rectangle are drawn**, and the grid's canvas is the decoration's
  own bounding box rather than the pane's. That is `task-1666`'s rule and `task-1765`'s, and it is
  what keeps a two-hundred-row page costing what twenty visible rows cost.

`cargo run --release -p quill-app --example vello_cost` gains a database arm, so the number is
measured again rather than asserted.

## 10. Tests

The bar is the repository's own: the control a person uses, the way an agent asks for the same thing
through the same code, and tests over both.

**`quill-db`, with no window and no server.**

- The framing fed the same bytes split at every boundary produces the same frames — `quill-chat`'s
  test, applied to a different protocol.
- A scripted server on `127.0.0.1:0` replays a real startup: `AuthenticationSASL`, the SCRAM exchange,
  `ParameterStatus`, `BackendKeyData`, `ReadyForQuery`. A second one replays a `SELECT`, a third an
  `ErrorResponse` with a `SQLSTATE`, a fourth a `NoticeResponse` in the middle of a result.
- SCRAM against **RFC 7677's own test vector**, so the arithmetic is checked against the standard
  rather than against itself. And a server signature that does not verify is a refusal.
- A server that says `N` to `SSLRequest` under `sslmode = require` is refused; under `prefer` it
  carries on.
- SQLite against real files in a temporary folder: create, introspect, query, page, update, insert,
  delete, and a table with no primary key that reports itself as not editable.
- The statement splitter over the awkward cases: a `;` inside a string, inside a dollar-quoted body,
  inside a line comment and inside a block comment.
- The pending changes produce the expected statements and parameters, including a value containing a
  quote, a newline and a backslash — the case that is a fault in every implementation that quotes by
  hand.

**The plugin, with no window.** The manifest parses and contributes a pane, a tab, a menu and a page;
`every_registered_provider_can_be_built` covers the new name; the older plugins ask for none of the
new keys; `view` answers the same numbers the drawing reads.

**The window, through the real widget tree.** `egui_kittest`, against a SQLite file the test builds:
press the rail button and the pane appears; expand a data source and its tables are listed; open a
table and the workspace tab shows its rows; type in `WHERE` and the rows narrow; edit a cell and the
Submit button appears; submit and the file on disk has changed.

**Screenshots**, accepted only after opening the image: the tree pane with a data source expanded, the
console with a result under it, the row editor with a pending change showing, the preview modal, the
new-data-source modal, and the settings page.

**The command line.** Each command driven against a real window and checked against the window's own
state read back, rather than against what the command said it did.

**By hand, against the real thing.** The PostgreSQL half is verified against the 17.2 server on this
machine — connect over SCRAM, list schemas and tables, run a query, page through a table, change a
row, submit, and read the change back with `psql`. That is the ticket's *"verify things work with test
Postgres and sqllite dbs"*, and a scripted server is evidence about the protocol rather than evidence
about the server.

## 11. Deliberately not here

Each of these is a refusal with a reason, not a gap.

**Engines other than PostgreSQL and SQLite.** The ticket says which two. A third is a module beside
the two and an arm in one `enum`; nothing about this design has to change for it, which is what makes
leaving it out cheap.

**A driver downloaded on demand.** Nothing in Quill is ever fetched.

**Manual transaction mode.** §6.3: an editor that can leave a transaction open holds locks while
nobody is watching.

**Editing a console result.** §6.3: a row that cannot be addressed cannot be changed safely, and
guessing at the table behind a join is how the wrong row gets updated.

**DDL editing** — creating, dropping and altering tables through dialogs. IntelliJ's is a large
surface, one dialog per object kind per engine, and every one of those things can be typed into the
console, which is where somebody who wants it will type it. `ddl <table>` shows the statement;
changing it is a statement you write.

**Import and export beyond CSV.** The extractor list in IntelliJ is a dozen formats and a template
language. Copying the grid as CSV covers what a person does with a result; `pg_dump` is a program that
exists.

**Charts, the geo viewer, transposed and tree view modes, the aggregate view.** Ways of looking at a
result that are not looking at the result.

**Foreign-key navigation.** `Related Rows` is genuinely good and genuinely a second navigation model;
it is the first thing to add after this ships.

**A second window's worth of session management.** IntelliJ's consoles can share a connection session;
here one data source is one connection and one worker.

## 12. Working beside `task-1776`

The themes plugin is being built in the same checkout at the same time, and the two tasks touch some
of the same files — `services/plugins.rs`, `theme/`, the settings dialog, the screenshot baselines.

So this work happens in a **git worktree of its own**, `C:\jason\dev\quill-1777`, on branch
`task-1777-database-plugin`, with its own `target/`. Nothing here writes into their working tree; the
branch merges when it is done, and the worktree and its build directory are deleted then.

What that leaves is the ordinary merge, and it is kept small on purpose. Almost everything this adds
is a **new file**: a crate, a plugin folder, two `services` modules, a `components` folder. The edits
to files task-1776 also touches are four lines in `plugins.rs` (two registry entries), one arm in
`plugin_ui::provider`, one entry in `bundled::ALL`, and two new functions at the end of
`theme::icon`. Each is an addition rather than a change, which is the kind of edit git merges without
help.

## 13. What building it changed, and what was measured

A design is a plan until something is built from it. Six things changed while this was implemented,
and each is here rather than quietly folded into the sections above.

**Two commands the design did not have.** `password <source> env <VARIABLE>` was missing outright:
`add-source` took a URL and there was no way at all to say *where* a password is except through the
dialog — so the first real run against PostgreSQL got the right refusal for the wrong reason, and an
agent could add a data source it could never connect. `add-source` now takes the variable name as an
optional third word too. And `confirm on|off` reaches the confirmation setting that previously existed
only as a tick box, which is the rule that a control a person has an agent has too.

**The confirmation asks a person and never a command.** `UiProvider::command` cannot press a button in
a modal, so raising one for `plugins run database query update …` would leave a command that could
never finish. `DatabaseExplorer::execute` keeps the confirmation for the button and `execute_now` is
what the command line calls; the guard that applies to **both** is the read-only switch, which is on by
default and enforced by the server rather than by Quill.

**Three faults in code that had already shipped**, all found by looking at the screenshots rather than
by a test failing:

- `Look::colouring_with` had no caller anywhere. The seam that colours a fenced block through the
  plugin claiming its language was built by `task-1767` and never plugged in, so the query console
  drew SQL in one flat colour — and so had **Agent-Chat's fenced code, for as long as it has existed**.
  It is wired now in the three places a plugin's `Look` is built.
- `plugins::PANE_ICONS` and `components::activity_bar::pane_icon` are two lists, and a name added to
  one and not the other falls through to the board icon: three identical marks in the rail and nothing
  failing. `every_named_icon_is_actually_drawn_rather_than_falling_back_to_the_board` is the test.
- `rusqlite`'s default open flags include `SQLITE_OPEN_CREATE`, so a mistyped path made an empty
  database and the tree showed a data source with nothing in it. A file is opened, never created.

**Two layout faults, also only visible in the picture.** The New Data Source dialog had five unlabelled
fields, and its Safety section drew over its own footer — a budget that did not add up, which is
exactly the fault `task-1771` records for the ticket modal. It has a label column now and a height
counted from its rows. And a SQLite file's path ran underneath the Edit and Remove buttons on the
Settings page, which is two lines a source now.

### 13.1 What was verified, against what

| Layer | What it proves | Where |
|---|---|---|
| 78 tests in `quill-db` | The framing, split at every byte boundary; SCRAM against RFC 7677's own vector; the statement splitter over the four things a `;` hides inside; the pending-change statements, including a value with a quote, a newline and a backslash | `crates/quill-db/src/**`, `tests/scripted_server.rs` |
| A scripted PostgreSQL | Connect, authenticate, read a result, an `ErrorResponse` with its `SQLSTATE`, a `NOTICE` mid-result, `sslmode=require` refused, an extended query, a row limit | `tests/scripted_server.rs` |
| 24 tests in the plugin | Laziness, one level at a time, the editing rule, the read-only refusal, the confirmation, the secret never written or answered back, every command answering or refusing with a sentence | `services/database/tests.rs` |
| 9 tests through the real window | The contributions, the tree pressed row by row, a grid, a console, `Output`, a failing statement, a pending edit written to a file, a read-only refusal, the dialog and the Settings page | `tests/screenshots.rs` |
| **A real PostgreSQL 17.2** | Everything above, against a server | by hand — below |

The by-hand run is the one the ticket asks for, and it was done twice: against Jason's own `ai`
database read-only, and against a `quill_db_test` database created and dropped for it.

Against `ai`: connected over SCRAM-SHA-256 in 53 ms; 213 items in `public`; `member`'s fourteen
columns with their real types and its `member_id` key; the composed `CREATE TABLE`; 37 rows;
`conversation_message`, which has **no primary key**, drawn read-only with the sentence that says so;
and a write refused with *"`ai` is read only, so `UPDATE` is not sent"*.

Against `quill_db_test`: two cell edits and a delete submitted as one transaction, then read back
**with `psql`** — a different program on a different connection — showing `Kind of Green`, `remastered`
and the third row gone. A value containing a quote, a newline and two backslashes round-tripped
exactly. A table with no key refused the edit in the same words the grid draws. A view reported itself
read-only for its own reason, and its DDL came back as `pg_get_viewdef`'s rather than as something
composed.

`_agent_output/task-1777-database-plugin/verify/postgres-window.png` is the window during that run.
