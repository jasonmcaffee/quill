# task-1795 — Database plugin fixes

> Get rid of safety checkbox. We don't want a safety check at all. Should be full access.
>
> For sql lite, there should be a file picker.
>
> Intellij should not be mentioned anywhere whatsoever in the settings, plugin, etc.
>
> "Where these plugins come from" text should be removed from all plugins.
> Database, agent-chat, agent-task, themes, etc plugins don't have an icon, and need one in the marketplace.
> The Install button should be Uninstall if the plugin is already installed.
>
> For database password, just have it below the user name. We don't want an env variable password.
> We want the password stored somehow secure. e.g. keychain, and whatever the equivalent is for windows.
> No idea what encryption option means. We want things stored securely.
>
> I should be able to double click an entry in the table, type to update, and see a save button that
> writes to the table for that value.
>
> CMD/ctrl + Enter should execute a sql statement.
>
> I should be able to right click the db or tables and see a popup to create a new table.
> Table creator modal should allow me to name the table, dropdown for column types, column names,
> see the generated sql on the right, etc.
> Add row doesn't provide a new row in the ui for me to type in.
>
> I cant paste a value in the file path for sql lite. It pastes to the document below. We need to fix
> this for all inputs.
>
> Create a tdd for these features, then fully implement.

Twelve items. Two of them turn out to be the same defect, and that defect is not in the Database
plugin at all — it is in every field in Unluminous — so it is the first section here.

## 1. A field only takes a click in the middle of it, and that is two of the reported faults

`controls::field_text_rect` is the function every field in Unluminous hands its rectangle to, and it was
written to solve a real problem: egui lays a `TextEdit` out at the **top** of the rectangle it is
given and `Frame::NONE` leaves no margin to push it down, so a field that handed over its whole
height put its words against its top edge. The answer was to hand over a strip one line tall, centred
in the field and inset from the left.

```rust
pub fn field_text_rect(ui: &egui::Ui, field: Rect, left: f32) -> Rect {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let width = (field.width() - left - 8.0).max(1.0);
    Rect::from_min_size(
        Pos2::new(field.left() + left, (field.center().y - row / 2.0).round()),
        Vec2::new(width, row),
    )
}
```

That strip is also the **only** part of the control the pointer can hit. Measured in the harness, on
the `File` field of the New Data Source dialog:

```
text rect = [[491.0 244.0] - [803.0 259.0]]     // 15 points tall, inside a 24 point field
```

So a 24-point field has nine points of dead height and eight points of dead width down its left hand
side, and a click there takes the keyboard nowhere. Reproduced, in `screenshots.rs`, by clicking four
points left of where the box begins — a point that is plainly inside the drawn field:

```
after a click in the padding, text_edit_focused = false
field value = ""                                // the paste never reached the File field
```

**The second half is why the text ends up in the file.** With no text box holding the keyboard,
`app::text_box_has_the_keyboard` is false, `self.focus` is still `Focus::Editor`, and
`editor_view::handle_input` reads the frame's `egui::Event::Paste` and inserts it into the document.
With a file open behind the Database pane, the same click and the same paste:

```
text_edit_focused = false
document = "helloPASTED"
```

which is exactly what the ticket reports. **`Ctrl/Cmd+Enter` in the SQL console fails for the same
reason**: it is guarded on `response.has_focus()`, and the console's `TextEdit` is inset eight points
inside a well, so a click in that margin leaves it without the keyboard.

### The fix

A field's **whole rectangle** claims the click and hands the keyboard to the box inside it. One
function, called by every field, so a field written later has it without asking — which is the reason
`field_text_rect` exists at all:

```rust
/// Give the whole of a field's rectangle to the text box drawn inside it.
///
/// Called **before** the box is added, so egui still gives the box itself any pointer inside its own
/// strip — the last widget that wants a pointer is the one that gets it — and this catches only the
/// padding round it.
pub fn field_takes_the_whole_rectangle(ui: &mut egui::Ui, field: Rect, id: egui::Id) -> egui::Response
```

Every field passes an explicit `egui::Id` to its `TextEdit` (`TextEdit::id`), which they have to do
anyway for this to name the right box, and which removes a second latent fault: an id derived from
egui's auto counter shifts when the number of widgets above it changes, and the source dialog draws a
different number of fields for PostgreSQL and for SQLite.

Applied to every input in Unluminous: `modal::field`, `controls::search_field_over`, the console's SQL
editor, the grid's `WHERE` and `ORDER BY`, the password field, and the new inline cell editor.

**A test per half.** `a_click_anywhere_in_a_field_hands_it_the_keyboard` and
`a_paste_into_a_plugins_field_never_reaches_the_document` — the second one is the reproduction above,
kept, because it is the one that would silently come back.

## 2. There is no safety check, and a data source is writable

The ticket is unambiguous: *"We don't want a safety check at all. Should be full access."*

| What goes | Where |
|---|---|
| The `Read only` tick box and the whole `Safety` section | the New Data Source dialog |
| `Ask before a console statement that changes rows`, and its `Safety` section | the Database settings page |
| `Modal::Confirm`, and the dialog it draws | `services::database`, `components::database::modal` |
| The refusal a read-only source raised on a writing statement | `DatabaseExplorer::execute_with` |

`config::DEFAULT_READ_ONLY` becomes `false`, so a source read from a file with no `read_only` line,
one added through the dialog, and one added by `add-source` are all writable.
`Configuration::confirm_writes` goes entirely, and so does the `confirm` command.

**What stays, and why.** `Source::read_only` itself stays on the model, and so does the `read-only`
command: it is the flag that becomes `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` and
`SQLITE_OPEN_READONLY`, so it is the only way to ask a server for a session that cannot write — a
thing an agent may deliberately want. It is off by default and no longer has a control anywhere, so
"full access" is what a person gets without touching anything. Deleting the mechanism as well as the
check would remove a capability the ticket did not ask to lose.

## 3. A SQLite file is chosen with a file picker

`rfd` is already in the tree — it is what `Open File` and `Open Folder` use. A `Browse…` button sits
beside the `File` field, opens `rfd::FileDialog` filtered to `db`, `sqlite`, `sqlite3` and `db3` plus
`All files`, and starts in the folder the field already names.

The field stays typeable, because a path that is pasted or typed is often quicker than walking to it,
and because a picker cannot be pressed by an agent. Nothing new is added to the command line:
`add-source <name> <path>` already takes a path.

## 4. Nothing mentions the tool this was measured against

The plugin was designed against JetBrains' own screenshots and it said so in about ninety places. The
ticket asks for none of it to be visible.

- **Every user-visible string**: `plugins/database/plugin.conf`'s `description` and `limitations`,
  the notes on the source dialog, the settings page, the preview modal, the tree's tooltips, and
  `unluminous-cli`'s own documentation for the database commands.
- **Every source comment** in `crates/unluminous-db`, `crates/unluminous-app/src/services/database`,
  `crates/unluminous-app/src/components/database` and `unluminous-cli`. A comment that names it is a comment
  that puts it back the next time somebody reads the file.
- **`README.md` and `documentation/`.**

What each of those sentences was *for* is kept — the reason a decision was made is the point of the
comment — with the product named as "the tool this was measured against", or the reason stated on its
own where the name was carrying nothing.

`no_shipped_text_names_the_other_tool` is a test over the shipped crates and the plugin manifests, so
it cannot come back unnoticed.

**Left alone deliberately:** `tasks/task-1777-database-plugin-tdd.md` and the research notes under
`_agent_output/`. They are the record of how the plugin was designed, are not shipped, and are not
reachable from the product; rewriting a design document to say something it did not say is worse than
leaving it. `CLAUDE.md` keeps its paragraph for the same reason, and because it is where the next
agent is told not to reintroduce the name.

## 5. The plugins page stops explaining where plugins come from

The `Where plugins come from` heading and its paragraph go from `components::plugins_page::detail`.
The one line above the button — `Installed. Its folder is under the settings folder…` — stays: that
says what the button did, not where plugins come from.

## 6. Four plugins get an icon

`database`, `agent-chat`, `agent-tasks` and `themes-bundle-1` have no `icon.png`, so the marketplace
list and their own page draw nothing where every other plugin has a mark.

Each is generated the way the other five were — the recipe is in `plugins/mermaid/icon.md`: a prompt
through the AI service's `POST /image-creation/generateImageToProjectFile`, then
`cargo run --example plugin_icon -- <source> crates/unluminous-app/plugins/<id>`, which keys the flat
background out, crops, squares and scales to 128 and to 32. Each plugin gets an `icon.md` recording
its prompt, so it can be made again without guessing.

The marks, each chosen to read at 32 by 32: a **stack of three discs** for Database, a **speech
bubble** for Agent-Chat, a **three-column board with cards** for Agent-Tasks, and a **circle split
into colour segments** for Themes.

`bundled::ALL` gains the four `include_bytes!` entries, and `every_bundled_plugin_carries_an_icon` is
the test that keeps a sixth from shipping without one.

## 7. Install becomes Uninstall

The page has one button, and it means three different things depending on state. It becomes two.

| | Primary button | Secondary button |
|---|---|---|
| Not on disk | `INSTALL` | — |
| On disk | `UNINSTALL` | `DISABLE` / `ENABLE` |

`PluginsOutcome` gains `uninstall: Option<String>`. `Plugins::uninstall(store, id)` removes the
plugin's folder under the settings folder and drops the installed copy, then loads the bundled one
back, so uninstalling a bundled plugin returns it to being bundled rather than removing the feature —
which is what `Install` means here in the first place. A plugin with no bundled copy behind it is
simply gone.

Removing a folder is guarded: the path is `store.folder()/plugins/<id>`, built from the id and never
from anything typed, and the id has to name a plugin that is actually installed.

## 8. The password sits under the user, and lives in the machine's own credential store

The `Password` section becomes one field, `Password`, immediately under `User` — where the ticket
asks for it. `In the variable` goes: `Secret::Environment` is no longer offered by the dialog.

**Where a password goes.** Typed once, written to the machine's own credential store under the entry
`unluminous-database-<source name>`, and the settings file records `password.keychain = <entry>` — the
name of the entry and never the value, which is the rule the file already keeps. Reading is at the
moment a connection is opened and the value is never held.

**Windows has a credential store, and Unluminous can now write to it.** `services::agent_tasks::keychain`
says in as many words that there is none — *"no such code was ever written, and a comment claiming a
secret is in a keychain when it is not is worse than no comment"* — and that was true, but the reason
given, that it *"cannot be tested from this machine"*, has not been true since Unluminous grew a Windows
build. It is `CredWriteW`, `CredReadW` and `CredDeleteW` in `Win32::Security::Credentials`, which is
one feature flag on the `windows-sys` dependency Unluminous already has — the precedent `services::recycle`
set for `SHFileOperationW`. The credential is `CRED_TYPE_GENERIC` persisted with
`CRED_PERSIST_LOCAL_MACHINE`, which Windows itself protects with DPAPI under the signed-in user. The
blob is copied out and `CredFree` is called on every path.

So the three platforms are `security` on macOS, `secret-tool` on Linux and Credential Manager on
Windows, behind the one `keychain::read/write/remove/is_set` that Agent-Tasks already calls — so
Agent-Tasks gains a Windows keychain from the same change.

`a_secret_round_trips_through_the_machines_own_store` is the test, and it runs on whichever of the
three this is.

**Backwards compatibility.** A `password.env` line already in a `sources.conf` is still read, so a
source somebody configured that way keeps working; it is described on the settings page as
`environment VARIABLE` and there is no way to make a new one. The `password` command keeps its `env`
form for the same reason and gains `set <source> <secret>`, which writes to the credential store.

**"No idea what encryption option means."** That row is `sslmode`, and the fault is that a person
cannot be expected to know that. It becomes **`Connection security`**, three buttons reading `Off`,
`If offered` and `Required`, with one line under them: *"Whether the connection to the server is
encrypted. Required refuses to connect without it. This is about the connection, not about how your
password is kept — that is always in this machine's own credential store."* The stored values are
unchanged, so an `sslmode` already written down still means what it did.

## 9. A cell is edited in place, and Save writes it

`Grid::editing` already exists on the model and nothing ever set it. It becomes
`Option<Editing { at: usize, column: usize, text: String }>`.

- **A double click on a cell** opens a `TextEdit` over that cell, filled with what the cell shows,
  holding the keyboard with the whole text selected. `Enter` commits, `Escape` cancels, and clicking
  another cell commits.
- **Committing records a pending change** — `Act::SetCell` — rather than sending a statement, which
  is the arrangement the grid already has and the reason Preview can show what will happen.
- **`NULL`** is what an empty box means only when the cell was NULL before; otherwise an empty box is
  the empty string. A cell is set to NULL from the toolbar, because the two cannot both be what an
  empty box means, and `unluminous_db::Value` keeps them apart all the way from the wire.
- **The button says `Save`.** `Submit 3` becomes `Save 3`, the menu entry `Submit Changes` becomes
  `Save Changes`, and the command keeps the name `submit` with `save` beside it, so nothing an agent
  already writes breaks.

## 10. Ctrl/Cmd+Enter executes

Fixed by §1 — the console takes the keyboard when it is clicked anywhere in it — plus one thing §1
does not cover: the chord is read while the SQL box has the keyboard, and `Enter` alone still inserts
a newline, which is what a console must do. `ctrl_enter_runs_the_statement_under_the_caret` is the
test, driving the real window.

## 11. A right click makes a table

**The menu.** A right click on a row of the tree opens a popup, drawn with `egui::Popup` in the same
frame `components::context_menu` uses, so it looks like the explorer's. `components::context_menu`
itself takes `actions::Entry` and returns an `actions::Action`, which is the window's vocabulary and
not a plugin's, so the plugin draws its own rows and answers its own `Act` — the same split every
other part of this plugin keeps.

| Right click on | Rows |
|---|---|
| a data source | New Table…, Open Query Console, Refresh, Disconnect, Edit Data Source, Remove Data Source |
| a schema | New Table…, Refresh |
| a table or a view | Open Data, New Table…, Show DDL, Copy Name, Drop Table… |
| a column | Copy Name |

`Drop Table…` is the one that asks first, because it is the one that cannot be undone. Nothing else
here asks.

**The modal.** `Modal::NewTable(TableForm)`, 720 by 560, two columns:

```
┌─ New Table ─────────────────────────────────────────────────────────┐
│ Data source  library            Schema  [main        ]              │
│ Name         [__________________]                                   │
│                                                                     │
│ Columns                              │ SQL                          │
│  ┌──────────┬──────────┬────┬────┐   │ ┌──────────────────────────┐ │
│  │ name     │ type   ▾ │ PK │ NN │   │ │ CREATE TABLE "main"."x" (│ │
│  │ id       │ INTEGER▾ │ ●  │ ●  │ ✕ │ │   "id" INTEGER NOT NULL, │ │
│  │ title    │ TEXT   ▾ │ ○  │ ●  │ ✕ │ │   "title" TEXT NOT NULL, │ │
│  └──────────┴──────────┴────┴────┘   │ │   PRIMARY KEY ("id")     │ │
│  [+ Add column]                      │ │ )                        │ │
│                                      │ └──────────────────────────┘ │
│                                    [Cancel]  [Create]               │
└─────────────────────────────────────────────────────────────────────┘
```

The SQL on the right is **the statement that will be sent**, composed by
`unluminous_db::sql::create_table` and redrawn as the form is typed into — the rule Preview already keeps,
that what is shown comes from the same call that acts.

**The type dropdown is the engine's own list.** `Engine::column_types()` answers `INTEGER, TEXT,
REAL, BLOB, NUMERIC, BOOLEAN, DATE, DATETIME` for SQLite and `text, varchar(255), integer, bigint,
boolean, numeric, real, double precision, date, timestamptz, uuid, jsonb, bytea` for PostgreSQL. A
type can also be typed, because a column type is a fragment of DDL and no closed list holds every one
— the same decision `WHERE` already embodies.

`create_table` quotes every identifier and refuses a name that is empty, and the statement is sent
through the ordinary query path, so a server that refuses it says so in its own words. The tree
refreshes the schema the table was made in, so the new table is there without pressing anything.

**The agent's half**, because a control a person has and an agent has not is not finished:

```
plugins run database new-table <schema.name> <column>:<type>[:pk][:notnull] …
plugins run database drop-table <schema.name>
```

and `new-table` with nothing after the name opens the modal, which is exactly the shape `add-source`
already has.

## 12. An added row is a row you can type in

`Pending::add` records a `Change::Add` and the grid draws `grid.rows.rows` — so pressing `Add row`
changed the pending count and put nothing on the screen. That is the whole of the report.

The grid draws `rows.rows.len() + pending.added().len()` rows. The extra ones come after the read
rows, numbered `+1`, `+2`, with the accent behind their row number, and every cell in them is edited
by the same double click as §9 — reading its value from `Pending::value_of` and showing an empty cell
as empty rather than as `NULL`. `Grid::row_of` already answers `Row::Added(n)` for them, so
`Act::SetCell` and `Act::DeleteRow` need no change at all.

`add_row_puts_a_row_on_the_screen_that_can_be_typed_into` is the test.

## 13. What is tested

Everything below drives the real window through `egui_kittest` in
`crates/unluminous-app/tests/screenshots.rs`, except where it says otherwise.

| Test | What would break without it |
|---|---|
| `a_click_anywhere_in_a_field_hands_it_the_keyboard` | §1, the root cause |
| `a_paste_into_a_plugins_field_never_reaches_the_document` | §1, the reported symptom |
| `ctrl_enter_runs_the_statement_under_the_caret` | §10 |
| `a_new_data_source_is_writable` (unit) | §2 |
| `no_shipped_text_names_the_other_tool` (unit) | §4 |
| `every_bundled_plugin_carries_an_icon` (unit) | §6 |
| `installing_then_uninstalling_leaves_the_bundled_plugin` (unit) | §7 |
| `a_secret_round_trips_through_the_machines_own_store` (unit) | §8 |
| `a_password_typed_into_the_dialog_is_not_written_to_the_file` (unit) | §8 |
| `double_clicking_a_cell_edits_it_and_save_writes_it` | §9 |
| `a_right_click_on_a_table_offers_a_new_table` | §11 |
| `the_new_table_modal_shows_the_statement_it_will_send` | §11 |
| `add_row_puts_a_row_on_the_screen_that_can_be_typed_into` | §12 |
| `every_command_is_offered_as_a_tool_in_both_shapes` (existing) | the two new commands reaching an agent |

And by hand, against a real SQLite file: add a source with a picked file, connect, make a table, add
a row, edit a cell, save, and paste a path into every field on the dialog.
