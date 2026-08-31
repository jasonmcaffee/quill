# agent-tasks-ui — the board's own interface, part by part

> the UI implementation is lacking many features, including the full page modal view of all the aspects
> of a task (description, terminal to communicate with an agent.
>
> i'm unable to create a task because all the required info doesn't have a ui.
>
> painting of windows seems to be slow
>
> there's no way to close the board view, and it should be draggable/repositionable.
>
> take screenshots of the board and task modal, and verify 100% of the ui is recreated and matches.

Four faults and one bar. This document is the specification for the whole of the Agent-Tasks interface,
part by part, against the browser board it replaces, and it is written because the first pass drew a
board that could be read and not used.

## 1. What the first pass got wrong

Four things, each named as a fault rather than as a gap, because each of them is a control somebody
looked for and did not find.

**A ticket had no full view.** The browser board opens a ticket as a modal filling the window: the
description on the left in a markdown editor, the todos under it, the terminal under those, the comments
under those, and every field of the ticket down the right. The pane drew a title, a description as
static text, the todos, the comments and nothing else. Reading a ticket was possible and working one was
not.

**A ticket could not be created.** `+ Add task` made a row and opened a detail that could set the title
and nothing else. A ticket needs a priority, an assignee, a model, an effort, a project and an epic
before it can be started, and none of the six had a control. The command line could set two of them.

**The board could not be put away or moved.** Every other panel in Quill has a header, and the header is
what closes it, drags it to another edge and opens its menu. The board pane had no header at all: once
shown it could only be hidden from the rail button, and it could not be moved.

**Painting was slow.** Measured below, and the cause was not the drawing.

## 2. What the browser board is, control by control

Read out of `apps/web/app/tasks/` rather than remembered. This is the list the implementation is
measured against, and §9 is the screenshot that says whether each row is done.

### 2.1 The rail

| Control | What it does |
|---|---|
| Collapse and expand | The rail is icons only, and expands to show a label beside each. Remembered between runs. |
| Board | The four lanes. |
| Messages | Slack. Absent here — §10. |
| Backlog | Tickets with no sprint. |
| Completed | Tickets in closed sprints. |
| Epics | The epics, with their colours. |
| Schedule | The cron rows and their next run. |
| Remote control | A link to the phone page. Absent here — §10. |

### 2.2 The board's header

| Control | What it does |
|---|---|
| `Current Sprint` | The heading, with the active sprint's name after it and the count of tickets. |
| Origin filter | Three buttons: tickets made here, tickets synced from JIRA, both. |
| Search | Narrows the lanes as it is typed. |
| `Sync JIRA` | Present only when JIRA is configured. Absent here — §10. |
| `+ Add Task` | Creates the row and opens the editor. |
| `Reconnecting` | Shown while the event stream is down. Not needed here: the board is a file this process has open, so there is no stream to lose. |

### 2.3 A lane and a card

| Part | What it shows |
|---|---|
| Lane header | A coloured dot, the lane's name, and the count. |
| Card title | The title, or `Untitled`. |
| Epic chip | The epic's name on the epic's colour, and the card's left edge in that colour. |
| JIRA key | A link, when the ticket came from JIRA. |
| Priority | One of three chevrons. |
| Key | `task-27`. |
| Todo count | A tick and `3/7`. |
| Comment count | A speech mark and the number. |
| Deep research | A button with three states. Absent here — §10. |
| Start | Launches the agent. |
| Agent badge | Round, the agent's initial, brighter while a terminal is attached. |
| Drag | Between lanes and within one. |
| `+ Add task` | At the foot of the New lane. |
| Agent chooser and play button | Under the New lane's heading. The chooser names which agent a new ticket goes to and pressing it names the other one; the play button starts the ticket at the top of New with that agent without opening it. Missed by the first survey and found by comparing the board against the reference capture. The chooser writes the same `agent` the Settings page writes, so the two cannot disagree. |

### 2.4 The ticket, in full

The modal the ticket asks for. Two columns inside one frame.

**The header**: the ticket's key in small text, the title as a field that saves when it loses focus, and
a close button.

**The left column**, in this order:

| Section | What is in it |
|---|---|
| Description | A markdown editor that fills whatever height the other sections leave, always open for writing rather than showing a rendered copy. A dictate button beside its label — absent here, §10. |
| Todos | `Todos 3/7`, collapsible and open, one row a todo with a tick where it is done. **Read only in the browser**, because the agent writes its own plan; here they can also be added and ticked, which is §5.3. |
| Terminal | Collapsible, with `attached` or `detached` beside the word, a `Resume session` button when a session exists and is not attached, and the terminal itself. |
| Comments | One block a comment with its author and when it was written, the body as markdown, `Edit` and `Send to terminal` on a human's own comments, and a box that posts one. |

**The right column**, in this order: the JIRA panel when the ticket came from JIRA; then `Status`,
`Assignee`, `Model`, `Effort`, `Project`, `Priority` and `Epic` as dropdowns, with `Model` and `Effort`
disabled for a ticket assigned to a person; then when it was created; then `Start Work`; then `Delete`.

### 2.5 The new ticket editor

The row exists before this opens, because `+ Add Task` creates it — which is what makes every field save
as it is typed rather than being held in a form. `Done` closes it and `Discard` deletes the row.

Title, then a grid of six dropdowns — Priority, Assignee, Model, Effort, Project, Epic — then the
description. The footer says `Starts saving as you type` and holds `Discard` and `Done`.

## 3. Where each of the three views lives

The browser has one window and one modal. Quill has a pane, a tab and a modal, and the same board has to
be right in all three. **The same board, drawn at three sizes**, which is one decision rather than three
implementations:

| Where | What fits | What it shows |
|---|---|---|
| The pane, 240 to 600 points wide | One lane at a time, scrolling sideways | The lanes, and a ticket **replacing** them when one is opened. A modal inside a 420 point column would be wider than its parent. |
| The tab, a whole editing area | Four lanes | The lanes, and a ticket beside them when the area is wide enough for both. |
| The modal, 1000 by 700 | Everything | The ticket in full, as §2.4 describes it. Opened from either of the other two. |

`components::modal` is what the modal is made of, which is the frame, the header, the body, the footer,
the rows, the fields and the tick boxes every other dialog in Quill is made of — and the dragging and
resizing that `modal::show` already owns. A tenth modal that drew its own header would be a tenth modal
that almost agreed with the other nine.

## 4. The pane gets a header, which is what closes and moves it

`components::dock::handle` is the one function that makes a rectangle a panel's drag handle: it takes the
presses, reports a drag with the pointer's position, and reports a right click as the panel's own menu.
The terminal tile calls it with its header rectangle, and so does the run tile and the debug tile. The
board pane now calls it too, with a 28 point strip along its top holding:

- the pane's label, from `pane.label` in the manifest;
- the count of tickets, in the dim colour, which is what the explorer's footer does;
- a close cross on the right, which is what `Settings` and every git dialog have.

That one call is the whole of "draggable and repositionable": the four drop bands, the strong rectangle
showing where it would land, the snap to an edge, and the `Move to` menu on a right click are all
`app::dock`'s already and none of them knows what a panel contains.

**The pane's own view buttons move under that header**, because a header is where a panel's name goes and
a row of view buttons above the name would read as the window's rather than the panel's.

## 5. What is being added

### 5.1 The ticket modal

`components::agent_tasks::modal` — the frame from `components::modal`, the two columns of §2.4, and one
rule about each half.

**The description is a plain multiline field, and that is less than this document first claimed.** The
claim was that `quill_core::Document` would hold it and `components::editor_view` would draw it, giving
it Quill's own editor: the same font, the same syntax colouring inside a code fence, the same undo, the
same caret. What is built is an `egui::TextEdit` in the window's own font, saving on every keystroke.

The difference is worth writing down rather than leaving as a claim nobody checked. `editor_view` draws
a `Document` that a **tab** owns: it reads the tab's scroll, its zoom anchor, its fold regions, its
marked passages, its blame column and its caret history, and every one of those lives on `OpenFile`.
Giving the description a real editor means giving the modal a tab's worth of state and a second owner
for the keyboard, and it is its own piece of work rather than a line of this one. §10 keeps it.

What the field does have is the thing that mattered: it is open for writing rather than showing a
rendered copy, it wraps, it takes as much height as the other sections leave, and it saves as it is
typed with no debounce, because writing one column of a local row is a hundred microseconds.

**Every field writes through one function.** `AgentTasks::edit_field` takes what changed and writes it,
so seven dropdowns are seven calls to one place rather than seven paths that agree today. `Model` and
`Effort` are **absent** for a ticket assigned to a person rather than disabled, which is Quill's rule and
is the one place this deliberately differs from the browser.

### 5.2 The editor for a new ticket

The same modal, opened in a different state: no key yet in the header, `Discard` and `Done` in the
footer, and the description below the fields rather than beside them. One component with a flag rather
than two components, because the browser's two share every field and differ only in what the footer says
— and a second copy of six dropdowns is a second place to forget one.

### 5.3 Todos that can be written

The browser's todos are read only, because the agent writes its plan. Here they can be added, ticked and
removed, and the reason to differ is that this board has no other way in: the browser has an API a person
can curl, and a pane in an editor has the command line, which is the same thing said differently. Both
are kept: `plugins run agent-tasks todo-add` and a box in the modal, through one function.

### 5.4 The terminal, in all three places

One session per ticket, drawn by `components::terminal_panel::grid`, which is what the terminal tile and
the run tile are both made of. So the ticket's terminal has the keyboard, the selection, the mouse
reports, the resize and the clipboard rules Quill's own terminal has. In the pane it is the bottom third;
in the modal it is a section that collapses; in the tab it is the bottom third of the ticket's half.

### 5.5 The JIRA panel on the ticket

The right column's first section, on every ticket rather than only on one that came from JIRA. The key is a
field somebody types and `Copy issue link` hands over the row's own `jira_url` when it has one and the key
itself otherwise. `plugins run agent-tasks jira-key <ticket> <key>` is the same thing from the command line,
and nothing here fetches anything.

The browser draws this panel only on a ticket JIRA supplied. Drawing it always is the one place this column
departs from the reference, and the reason is that there is no sync: a panel that appeared only once a key was
set could never be the thing that set one.

### 5.6 A comment can be changed after it is posted

`Edit` on a person's own comment turns it into a field with `Save` and `Cancel`; `comment-edit <id> <body>` is
the same from the command line, and the reported comments now carry their ids so an agent can name one.

**An agent's comment cannot be changed**, and the refusal is in the store rather than in the button. A comment
an agent wrote is the record of what it said, and a history anybody can rewrite is not evidence. `created_at`
does not move when a comment is edited, because that is when it was said.

### 5.7 The board can be driven from the keyboard

The arrow keys move a ring from card to card and `Enter` opens the ticket it is on. The ring is a lane and a
row rather than a ticket's id, so a card moved or deleted under it clamps to the last row there is instead of
leaving the keyboard on nothing. Empty lanes are stepped over, the ends do not wrap, and a lane the board has
scrolled past is scrolled into view when the ring crosses to it.

Three things have to be true before a key is taken: the window says this plugin holds the keyboard, which
`Look::has_the_keyboard` carries; no text box has it; and no modal has it. Clicking the board is what gives it
the keyboard, and doing so takes the keys off a ticket's terminal. A key the board acts on is **consumed**, or
whatever draws after it sees the same press — the `Enter` that opened a ticket was still in the frame's input
when the modal drew, so the modal took it as its own confirm and shut again in the frame it opened.

### 5.8 Where the authentication key can be kept, and where it cannot

`security` on macOS and `secret-tool` on Linux. **There is no Windows keychain**, and the Settings page says so
on Windows rather than offering a field that cannot save: the key field, `Save key` and `Clear` are absent
there, and what is drawn instead is the sentence naming the platform. The code used to claim PowerShell's
credential store was used on Windows; no such code was ever written.

What the key ends up in is the agent process's environment, which any program running as the same user can
read. That is the same reach such a program would have to run `security find-generic-password` itself, so the
keychain is not being undermined: it protects the key from a copied settings file and from other users, and it
does both. Both agents read their key from the environment and neither reads it from a file, so there is
nothing else to hand them. Nothing Quill writes carries the value — the settings file holds the **name** of
the entry, and `SessionSettings` has a hand written `Debug` that prints its variable names and not their
values, so a log line or a stray `dbg!` cannot leak one.

## 6. Why the painting was slow, measured

`cargo run --release -p quill-app --example board_cost` reports the frame, and the numbers are what this
section is for rather than a guess.

Three things were being done once a frame that did not need to be done at all:

**The board was cloned.** `lanes::show` called `board.board().clone()`, which copies every ticket's title
and description. On the 27 ticket board this is 8 KB a frame; on a 500 ticket board it is 200 KB a frame,
at 60 frames a second. It reads, so it now borrows.

**Two SQLite queries ran inside the draw.** Backlog and Completed each ran a query per frame while their
view was showing. They are read when the view is chosen and when a command changes something.

**The whole scrollback was rebuilt per terminal per frame.** `written_text(None)` returns every line a
session has ever printed, and it was called once a frame per ticket to answer whether anything had moved.
It reads the last screenful now.

And one thing was asking for frames it did not need: a terminal that was merely *alive* asked for a
repaint every 120 ms, so an agent sitting at its prompt kept the window drawing for ever. The session has
the window's waker, which is how the terminal tile does it, so it asks for a frame when it prints.

## 7. What has to stop reading what

Three things read the board's state and have to keep agreeing after this:

- **The card** reads `session_id` for the badge, and it must read whether a terminal is running **now**
  instead: the row keeps its session id for ever, because that is what a resume names.
- **The rail** and the dock read `plugins::Surfaces`, and the pane's header is a third reader of the same
  value. All three ask `PluginUi`, which is the one place it is worked out.
- **The modal** and the pane both draw a ticket, and both read `AgentTasks::detail`. One value, so the
  ticket open in the pane is the ticket the modal opens.

## 8. Tests

**With no window.** Which fields an assignee allows, which are absent for a person, and that
`edit_field` writes exactly the column it names and no other.

**Through the real widget tree.** `+ Add Task` opens the editor; typing in the title changes the card;
each of the six dropdowns changes the row; `Done` closes it and the ticket is on the board; `Discard`
deletes it. In the modal: the description is typed into and saved, a todo is added and ticked, a comment
is posted, `Start Work` launches, `Delete` removes the ticket. The pane's header closes it, and dragging
that header to the bottom docks it there.

**Screenshots**, each opened and looked at before it is accepted: the board in the pane, the board as a
tab, the ticket modal with every section, the new ticket editor with its six dropdowns, the modal's
terminal running, and the board beside the reference capture the ticket carries.

**Measured**, in `examples/board_cost.rs`: the frame cost with the board showing, which is the number §6
is about.

**Added after the second review**: the quick launch under the New lane cycles the agent and starts the top
ticket; `Edit` on a comment changes it and the store refuses an agent's; the JIRA key is recorded and its link
copied; the arrow keys move the ring, step over empty lanes, stop at the ends and `Enter` opens what the ring
is on; and `SessionSettings` prints its variable names and never their values.

## 9. The bar

Every row of the tables in §2 is either drawn, or named in §10 as absent with its reason. A screenshot of
the board and a screenshot of the ticket modal are compared against the browser board's own capture, and
the comparison is written down rather than asserted.

## 10. Deliberately absent, each with its reason

- **Messages, and Slack.** A chat product that shares a repository with a board. `tasks/agent-tasks-plugin-tdd.md` §10.
- **Remote control.** A phone attaching to a terminal needs a server.
- **Deep research.** One command typed into an agent, which `plugins run agent-tasks send` already does.
- **Sync JIRA, and the origin filter.** No sync, so no origins to filter and no button. Both come back together.
- **Dictate.** Quill has no audio.
- **Image paste into a description.** Quill draws pictures and resolves them in a markdown preview; where a pasted image is written is a decision this does not need to make.
- **`Model` and `Effort` disabled rather than absent.** Quill's rule is absent, and this follows Quill.
- **The JIRA panel only on a JIRA ticket.** Drawn on every ticket instead, §5.5, because with no sync a panel that appeared only once a key was set could never set one.
- **A Windows keychain.** §5.8. Driving `[Windows.Security.Credentials.PasswordVault]` through PowerShell cannot be tested from this machine, and untested code that handles a secret is not worth having. The page says so on Windows rather than offering a field that does nothing.
- **The description in Quill's own editor.** §5.1 says what it would take: `components::editor_view` draws a `Document` a **tab** owns, reading its scroll, its zoom anchor, its folds, its marked passages, its blame column and its caret history, all of which live on `OpenFile`. The description is a multiline field until that state has somewhere to live that is not a tab.
