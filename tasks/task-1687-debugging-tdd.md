# task-1687 — debugging: breakpoints, stepping and inspection, spoken over the Debug Adapter Protocol

## 1. What was asked

> We want our quill editor to support line breaks and debugging for the languages we support,
> stepping into, stepping out, next step, etc. We also want variable/object/etc inspection,
> similar to IntelliJ, where we can view, modify, etc values. We want our functionality to mimic
> IntelliJ's.

"Line breaks" here is read as **line breakpoints** — the red dot in the gutter that pauses the
program on that line — because that is the thing stepping and inspection are built on and the thing
IntelliJ's debugger opens with. So the ask is: set breakpoints in the gutter, run a program under a
debugger, step over, into and out of calls, see the call stack, and view and change the values of
variables while the program is paused — behaving the way IntelliJ behaves, in the languages Quill's
plugins already claim.

This document is the design. `task-1689` is the implementation, and it is built: the whole of §4 to
§12 shipped in Quill 0.14.0.

Four things the implementation settled differently from the text below, each recorded where the code
makes the choice rather than only here:

- **§2.2's lifecycle summary has `setBreakpoints` before `launch`, and it has to be the other way.**
  The `initialized` event is what says an adapter will accept breakpoints, and no adapter sends it
  until `launch` has arrived; sending them earlier gets them refused by lldb-dap and dropped by
  js-debug. `quill_dap::session`'s own comment records it.
- **§6.2's gutter column was already spent.** `task-1686` put the folding arrows in the 12 points
  `components::gutter` had reserved, and a second control cannot share twelve points with one that
  already fills them. The dot is drawn **over the line number** instead, which is what IntelliJ does
  and what costs the gutter no width at all — so no accepted screenshot moved sideways. With the line
  numbers switched off there is nothing to draw over and a column of its own is reserved.
- **§9's debug button goes *above* the run one, not below it.** The rail is read bottom upwards, so
  "below" would have taken the bottom-left corner — which is where `task-1658`'s reference capture
  puts the **terminal** and where a dozen accepted screenshots have it. The older promise wins.
- **`runInTerminal` is answered with `success` and no process id.** §7.2 says Quill replies with the
  process id; a pseudoconsole hands back a console rather than a child and alacritty's pty layer does
  not surface it. The specification makes `processId` optional for exactly that reason, and it is what
  lldb-dap's own comm-file scheme and js-debug both expect.

And one thing the implementation had to add that the design did not name: **"stopped" and "there is
something to look at" are not the same instant.** A `stopped` event is one thing and the four requests
it causes are another, so there is a window of a few round trips in which the session is paused and
knows nothing about where. Measured against a real CodeLLDB, `debug status` in that window answers
with a null line and `debug variables` with an empty list — both of which look exactly like a program
that stopped somewhere uninteresting. `DebugState::is_ready` is the distinction, and it is what
`--wait-for-pause` waits for.

## 2. What the surveyed editors do

### 2.1 IntelliJ — the behaviour the ticket names

IntelliJ's debugger is the same shape in every JetBrains IDE, and the ticket asks for that shape.
A click in the gutter puts a red dot on a line. A run configuration has a Run button and a **Debug**
button beside it — debugging is not a different kind of configuration, it is the same configuration
started under the debugger, on `Shift+F9`. When a breakpoint is hit the editor jumps to the line,
highlights it, and the Debug tool window shows three things side by side: the **frames** of the
call stack (with a thread chooser above them), the **variables** of the selected frame as a lazily
expanded tree, and the **watches** — expressions re-evaluated at every stop. Clicking a frame moves
the editor and the variables to that frame.

Stepping is a small set of keys that never change: **Step Over** `F8` runs the current line and
stops on the next; **Step Into** `F7` enters the call on the current line; **Step Out** `Shift+F8`
finishes the current function and stops in the caller; **Resume** `F9` runs to the next breakpoint;
**Run to Cursor** `Alt+F9` runs to the line the caret is on. Beyond those it has Smart Step Into
(`Shift+F7`, choose which of several calls on one line to enter), Force variants that ignore
breakpoints on the way, and stepping filters that skip library classes.

Inspection is more than viewing. **Set Value** on a row of the variables tree changes the variable
in the running program. **Evaluate Expression** `Alt+F8` takes any expression and evaluates it in
the current frame. Recent versions also paint **inline values** — each local variable's value at
the end of its line, in a quiet colour, while the program is paused.

Breakpoints themselves carry data: a **condition** (pause only when this expression is true), an
**enabled** flag, and "evaluate and log" — a breakpoint that prints instead of pausing, which the
rest of the world calls a logpoint. Breakpoints persist with the project.

### 2.2 VS Code — where the protocol comes from

VS Code is where the **Debug Adapter Protocol** was written, for the same reason the Language
Server Protocol was: so that one editor-side client can drive any language's debugger through a
common wire format. A *debug adapter* is a separate program that speaks DAP on one side and drives
a real debugger on the other. The protocol is Content-Length framed JSON — the LSP wire format —
with three message kinds: requests from the editor, responses from the adapter, and unsolicited
events (stopped, output, terminated) from the adapter. A session is: `initialize` (capabilities are
exchanged, so a client never guesses what an adapter can do), `setBreakpoints` per file,
`configurationDone`, `launch` or `attach`, then a `stopped` event each time the program pauses, and
the state is read as a lazy waterfall — `threads` → `stackTrace` → `scopes` → `variables`, where
anything structured is a `variablesReference` integer the client asks about only if the person
expands that row. `setVariable` changes a value; `evaluate` answers watches and the expression box;
`next`, `stepIn`, `stepOut`, `continue` are the stepping. Breakpoints are set by **full replacement
per file**, and the adapter answers with where each one actually landed and whether it is
`verified` — an honest protocol, which suits Quill.

Two details matter to this design. The adapter is started as a child process and spoken to over
**stdio**, or is a server the client connects to on a **port** — both are common, and Quill needs
both. And the protocol has a reverse request, **`runInTerminal`**, by which the adapter asks the
*editor* to run the debuggee in the editor's own terminal — which is exactly what Quill's run tile
is.

### 2.3 Zed — the same decision made by a Rust editor

Zed shipped its debugger in 2025: eight months, 977 commits, 25k lines. It is a DAP client — Zed
implements the editor side once and the adapters implement the debugger side — with a two-layer
architecture, a data layer that owns the session state and caches and invalidates adapter
responses, and a UI layer that asks the data layer only for what is on screen, so the lazy
`variablesReference` model is preserved end to end. Rust, C/C++, JavaScript, Go and Python work out
of the box. Zed also built *locators* that turn a build task into a debug target automatically, and
paints inline values by matching tree-sitter's variables against the top frame's scope — a reminder
that inline values are the client's own work; DAP has no request for them.

### 2.4 Helix — the smallest honest client

Helix — a terminal editor in Rust with no plugin runtime — carries a working DAP client:
`helix-dap` speaks the framed JSON over stdio or TCP, external adapters translate to gdb and lldb,
and breakpoints, stepping and variable inspection all work. It is the existence proof that a DAP
client is a bounded piece of work for an editor of Quill's size, nothing like the open-ended cost
that made task-1675 refuse a language-server client.

### 2.5 What was rejected

**One integration per debugger.** Driving gdb's MI protocol for native code, the V8 inspector's
CDP for Node and pydevd's own wire for Python would be three clients, three test rigs and three
sets of faults, and the fourth language would cost a fourth. This is the choice DAP exists to
make unnecessary, and the debuggers themselves have already made it: lldb ships `lldb-dap` in the
LLVM distribution, Python's debugpy *is* a DAP adapter, Microsoft's js-debug — the actual debugger
inside VS Code for Node — publishes a standalone DAP server, and Go's delve serves DAP natively.
Speaking DAP is not adding a translation layer; it is speaking the native protocol of the programs
that already exist.

**Embedding a debugger as a library.** Linking liblldb would put a very large C++ dependency
inside the editor, behind an interface LLVM does not promise to keep stable, where a debugger crash
takes the editor with it — the same reasons `plugin.kind` refuses dynamic libraries — and it would
buy native code only; Node and Python would still need something else.

**A language-server-style refusal.** task-1675 weighed a language server client and chose a
syntactic index instead, and it is worth saying plainly why that verdict does not carry over. Go to
definition had a cheaper tier that answers instantly and deterministically from the token stream.
Debugging has no such tier: there is no syntactic way to pause a running process. A debugger is
inherently a separate program holding the debuggee, so the costs that made a language server
unattractive — a process per language, found on the machine, answering when it pleases — are not
costs DAP adds; they are the nature of the feature. What *can* be kept from task-1675 is the
testing answer: the protocol client is tested against a scripted adapter with fixed bytes, exactly
as the terminal's screenshot tests feed a session with no shell behind it, so when a real adapter
answers is never something a test waits on.

**Writing our own debuggers.** Quill draws its own Mermaid because a diagram is arithmetic.
A debugger for one language is a career; for three it is not a serious option.

## 3. The shape of the design

A new crate, **`quill-dap`**, speaks the protocol: the framing, the typed messages, the session
lifecycle, and a worker thread arranged exactly as `quill_git::Worker` is — requests down a
channel, replies and events back up, a waker that asks the window to draw. It has no user
interface dependency and its tests run against scripted adapters with no real process.

**Which debugger a language uses is data in the plugin, and the debugger itself is code in
Quill** — the rule `language.renders` and `run.project` already follow. A manifest names
`debug.adapter = lldb`; the name is checked against a built-in registry and an unknown one is
refused with a message; Quill's own code knows how to find and start each adapter it ships
knowledge of. Nothing in a plugin is executed and nothing is ever fetched.

**Debug is Run, under a debugger.** A debug session starts from the same `Configuration` the play
button starts — same command, same directory, same environment — which is IntelliJ's own model and
is the seam task-1683 §12 said a debugger would need. The debuggee's output lands in the run
tile's real terminal through `runInTerminal`. The debug tile is the third occupant of the bottom of
the window, built from the furniture the run tile and the terminal tile already share.

Breakpoints live where highlights live — in `quill_core`, inside the `Document`, as offsets that
move with the text — and persist beside the project in `.quill`, owned by the open `Document` or by
the store under the one rule every awkward case in Quill is already settled by.

Everything is an `Action` and a row in the CLI catalogue, so the whole feature is reachable from
the menus, the keyboard, `quill-cli` and the MCP server on the day it lands — which for this
feature is worth more than usual: an agent that can set a breakpoint, run to it and read the
variables is an agent that can debug a program rather than guess about it.

## 4. The protocol, and the crate that speaks it

### 4.1 The wire

`quill-dap` implements the base protocol: ASCII headers, `Content-Length: N`, `\r\n\r\n`, then N
bytes of UTF-8 JSON. Three message kinds — `request`, `response`, `event` — each carrying a `seq`;
a response repeats its request's seq as `request_seq` and says `success`. The codec is two pure
functions, bytes to messages and messages to bytes, tested on transcripts with no process behind
them, including the torn-buffer cases (a frame split mid-header, two frames in one read) that a
pipe will produce in practice.

Messages are typed with `serde`, and only the messages Quill uses are typed: `initialize`,
`setBreakpoints`, `setExceptionBreakpoints`, `configurationDone`, `launch`, `disconnect`,
`terminate`, `threads`, `stackTrace`, `scopes`, `variables`, `setVariable`, `evaluate`, `continue`,
`next`, `stepIn`, `stepOut`, `pause`, and the events `initialized`, `stopped`, `continued`,
`output`, `breakpoint`, `terminated`, `exited`, plus the `runInTerminal` reverse request. Fields
Quill does not read are not modelled — the protocol is large and additive, and `serde` ignores
what it is not asked for.

### 4.2 The session lifecycle

A `Session` is a small state machine: `Starting` (adapter spawned, `initialize` sent),
`Configuring` (the `initialized` event arrived; breakpoints are being sent, then
`configurationDone`), `Running`, `Paused` (a `stopped` event named a thread and a reason),
`Ended` (terminated or the adapter died). The capabilities from `initialize` are kept on the
session, and every optional feature asks them first — `supportsSetVariable`,
`supportsConditionalBreakpoints`, `supportsLogPoints`, `exceptionBreakpointFilters` — so Quill
never sends what an adapter did not offer, and a control whose capability is absent is absent (the
rule the `F` button already follows).

On `stopped`, the client requests `threads`, then `stackTrace` for the stopping thread, and the
top frame's `scopes` and first level of `variables`, unprompted — the four requests every stop
needs — and everything deeper waits until a row is expanded. Every `variablesReference` is valid
only while the program stays paused; the cache of fetched rows is cleared on `continued` and on
every stepping request, which is Zed's invalidation rule and the protocol's own.

Ending is soft then hard, the shape `RunPanel::stop` already has: `terminate` first (the graceful
request, honoured by adapters that can), then `disconnect`, and if the adapter process itself will
not die, kill it — it is a child process Quill owns, the same as any run.

### 4.3 The thread

The window never blocks on an adapter. `quill_dap::Client` is arranged as `quill_git::Worker` is:

- `start(adapter: AdapterCommand, waker: Waker) -> Client` spawns the adapter process (with
  `CREATE_NO_WINDOW` on Windows, as `quill-git` runs git) or connects to its port, and starts one
  reader thread that parses frames and pushes `Reply` values onto an `mpsc` channel, calling the
  waker after each — the same `Arc<dyn Fn() + Send + Sync>` the terminal and git already take.
- `send(request)` writes a frame; `poll() -> Vec<Reply>` drains once a frame at the top of the
  frame, where `Worker::poll` and `Session::pump` are already called.
- Replies are correlated by seq on the window side, and large payloads (a stack, a variable page)
  are boxed, as `Reply::Snapshot(Box<Snapshot>)` is, to keep the enum small.

One debug session per window. IntelliJ runs several; the first version of this does not, and says
so in §13 — the state below is one `Option<DebugState>`, not a collection, and everything is
simpler for it.

## 5. Where a debugger comes from

### 5.1 The registry, and what a plugin says

One new manifest key, off unless a language asks for it:

```
debug.adapter = lldb
```

checked in `plugins::parse` against a built-in list, with the refusal message in the house shape:

```rust
pub const DEBUGGERS: &[&str] = &["lldb", "node"];
// debug.adapter is `gdb`, and this version of Quill drives lldb, node
```

The rust plugin names `lldb`; javascript and typescript name `node`; css, mermaid and every plugin
that names nothing simply have no debugger, and every debug control is **absent** for their files —
`file_kind`'s rule, answered by one new question, `Plugins::debugger_for(path)`, which the menus,
the title bar and the CLI all ask so none of them can disagree.

The most a third-party manifest can do is name an adapter that shipped in the binary, visibly —
the same ceiling `run.project` has. Nothing in a plugin is executed.

### 5.2 Finding the program, and the honest refusal

Quill fetches nothing, ever — the preview's rule, kept here. Each registry entry knows the
commands it will look for on `PATH`, in order, and a settings key overrides the lot
(`debug.lldb = C:\tools\codelldb\adapter\codelldb.exe` in the settings file, the
`terminal.shell` pattern: empty means "what this machine has").

Pressing Debug with no adapter on the machine is not an error dialog and not a dead button: the
session refuses to start and the status bar says what was looked for and where it comes from —
`Debugging rust needs lldb-dap or codelldb on PATH. lldb-dap ships with LLVM.` — one sentence,
built from the registry entry, in the place every other message already lands. Nothing invents a
message once a session *is* running: an adapter's own error responses and `output` events are shown
as git's stderr is shown, because a debugger explains itself better than Quill could.

### 5.3 The adapters this version knows

**`lldb`** — for Rust and native code. Looks for `codelldb` first, then `lldb-dap`. CodeLLDB
(single-session mode, a port on localhost) carries Rust-aware formatters and is the better answer
where it is installed; `lldb-dap` (stdio) ships inside every LLVM distribution and is the floor.
One honesty that must reach the page rather than be hidden: with the MSVC toolchain that rustup
installs by default on Windows, LLDB reads PDB debug info incompletely — breakpoints and stepping
work, and some enums and collections render poorly. The TDD states it, the docs state it, and the
variables tree shows what the adapter says rather than pretending. (Rust 1.85+ ships LLDB
formatters that improve this; the GNU toolchain's DWARF is fully readable.)

**`node`** — for JavaScript and TypeScript. Microsoft's js-debug, run as
`node <path>/dapDebugServer.js <port>` and connected to on localhost. It is a server rather than a
stdio child, which is why `AdapterCommand` has both shapes. It debugs Node programs and TypeScript
through source maps — the same programs `run.file = node {file}` and `npx tsx {file}` already run.

**What about Python?** Quill ships no Python plugin today, so the registry ships no `python`
entry — an entry no manifest can name would be dead code. The day a Python plugin is written,
`debugpy` (`python -m debugpy.adapter`, stdio, and it *is* the protocol natively) is the entry to
add, and §13 records it.

## 6. Breakpoints

### 6.1 Where they live

`quill_core::breakpoints`, inside the `Document`, built as `highlights` is built: a sparse set
sorted by position, each holding the **byte offset of its line's start**, an `enabled` flag, and
optional `condition` and `log_message` strings. It lives in the document so that `insert` and
`remove_range` — the only two places that know bytes moved — shift it in the same lines that
already shift `chars` and the highlight marks, which is what makes a breakpoint stay on its line
while the file is edited above it. It rides the undo `Snapshot` as highlights do, because undo
restores a state. Toggling one is **not an edit**: revision bumps so the window repaints, `modified`
stays false, no undo step — the highlights' rule, unchanged.

Lines are derived, not stored: the document answers "which 1-based line is this offset on" when
the adapter needs line numbers, and the adapter's answers come back through the same conversion.
A stored line number would be wrong after the first edit; a stored offset is maintained by the
machinery that already exists.

### 6.2 The gutter

The gutter's `GAP` — the 12 points `components::gutter` already reserves at its left edge, whose
comment says it is where a control like this would go — becomes the breakpoint column. A **left
click** in the gutter toggles a breakpoint on that line: today the gutter takes only
`secondary_clicked` over its whole area, so the click is new behaviour, taken per-row the way the
blame cell already takes one (`ui.interact` with an id keyed by paragraph). The dot is drawn on a
paragraph's first visual line, filled in `color::CLOSE`'s family — a drawn circle in
`theme::icon`'s manner, not a letter — dimmed hollow when disabled, and hollow with a quiet ring
while the session has not verified it (§6.3). A breakpoint with a condition carries a small mark
(IntelliJ's question-mark badge, drawn not lettered).

Right click on a row with a breakpoint extends the existing gutter menu (`actions::gutter_menu`)
with `Edit Breakpoint...`, `Disable Breakpoint`, `Remove Breakpoint` — entries about the row under
the pointer, the rule the text menu and terminal-tab menu already follow. `Edit Breakpoint...`
opens a small modal built from `components::modal` — a field for the condition, a field for the
log message, a tick box for enabled — and Enter answers it, because `modal::footer` already makes
that true for every dialog built from the furniture.

### 6.3 What the adapter says back

`setBreakpoints` is full replacement per file, and the adapter answers with where each breakpoint
really landed and `verified`. Quill draws the adapter's answer, not its own hope: a breakpoint the
adapter moved to the next statement is drawn where the adapter put it for the life of the session,
and one the adapter could not bind — a line with no code, a file not in the build — stays hollow.
This is the honesty rule task-1675 set (`Confidence::Likely` stays marked all the way to the
screen), applied to a protocol that was designed for it.

Breakpoints are sent for every file that has any, at session start, and re-sent for a file
whenever its set changes while a session is live — toggled, disabled, edited — using the
document's current line numbers at that moment. Editing *text* during a session does not re-send
(the running program's code has not changed; the adapter's positions stand), which is what every
surveyed editor does, and the dots follow the text so the picture stays right for the person.

### 6.4 Conditions, log messages, and exceptions

A condition and a log message are **data in the `setBreakpoints` request** — `SourceBreakpoint`
carries both fields — so the adapter does the evaluating and the logging, and Quill's cost is two
optional strings and the modal that edits them. They are offered only when the capabilities say
`supportsConditionalBreakpoints` / `supportsLogPoints`; otherwise the fields are absent, not dead.

Exception breakpoints — IntelliJ's "break on exception" — come from the adapter too:
`exceptionBreakpointFilters` in the capabilities is a list of named filters (debugpy offers
raised/uncaught, lldb offers throw/catch), shown as tick boxes in the debug tile's header flyout
and sent back through `setExceptionBreakpoints`. Quill holds no list of its own; an adapter that
offers none gets no control.

### 6.5 The file beside the project

`.quill/breakpoints.conf`, in the numbered form `run-configurations.conf` established, written by
`store::Values` with the usual header, read and written **only by the released binary**:

```
# The breakpoints in this project. Written by Quill, and safe to edit by hand.
breakpoint.1.path = src/main.rs
breakpoint.1.offset = 1204
breakpoint.1.enabled = true
breakpoint.2.path = backend/server.js
breakpoint.2.offset = 88
breakpoint.2.condition = attempts > 3
```

Paths relative to the project, so a project that moves keeps its breakpoints — `file_marks`'
reason. A block missing `path` or `offset` is dropped whole, the `run.N.*` rule. And the ownership
rule is stated once and settles every case, verbatim from highlights: **a file that is open is
owned by its `Document`, and every other file is owned by the store.** `QuillApp::change_highlights`
gains a sibling, `change_breakpoints`, the one place the choice is made; the every-frame
reconciliation rides the same revision comparison `remember_the_marks` already makes, so it costs
an integer compare per tab. Offsets read from disk are clamped to the file's length as
`set_highlights` clamps, so a file rewritten outside Quill gives a misplaced dot rather than a
panic — and the adapter's `verified` answer then says so honestly.

## 7. A debug session

### 7.1 Debug is Run, under a debugger

`Debug <name>` sits beside `Run <name>`: same `Configuration`, same snapshot-not-name rule, same
one-instance rule. Starting it spawns the adapter (§5.2), runs the lifecycle (§4.2), and the
`launch` request carries the configuration's parts — the program and arguments from
`split_command`, `cwd` from `working_directory(root)`, `env` from `environment()` — translated by
the registry entry into the adapter's launch shape (each adapter names these slightly differently;
that knowledge is Quill's, in the registry, never the plugin's). `Debug Current File` exists
exactly where `Run Current File` exists *and* the language names an adapter — both questions asked,
so a `.rs` file (no `run.file`, deliberately) offers neither, and a `.css` file offers nothing.

Which configuration can be debugged is honest: a configuration whose program is `cargo` or `npm`
runs a build tool, not the program, and handing `cargo` to lldb would debug cargo. The first
version keeps this simple and truthful — `Debug` launches the configuration's command as the
debuggee. For `cargo run` that is wrong, and rather than being cleverly wrong, the registry's
translation for `lldb` refuses it with a sentence: debugging a Cargo project means naming the
built binary in a configuration (`target\debug\myapp.exe`), and the message says exactly that.
Zed's locators — deriving the binary from the build system — are the right eventual answer and are
§13 material, recorded with what they cost.

### 7.2 Where the program's output goes

The debuggee runs **in the run tile**. When the adapter sends the `runInTerminal` reverse request
— lldb-dap, js-debug and debugpy can all ask for it — Quill answers it by starting the requested
command through the run tile's own path (`RunPanel::start`'s machinery, a real
`quill_terminal::Session` at the size `run_grid_size()` already computes), and replies with the
process id. The program gets a real ConPTY, its colours and its interactivity, and the run tile's
rules — opened at final size, never resized while starting, exit code in the strip — all hold
because it *is* a run. When an adapter does not ask (launch in the adapter's own process), the
`output` events are fed into a detached grid through `Session::feed`, which the screenshot tests
already prove renders anything.

The stop/rerun/exit-code furniture of the run tile stays the run tile's. The debug session's own
lifecycle rides the DAP events: `terminated`/`exited` end the session and the tile says so.

### 7.3 Stepping

Five requests, five keys, IntelliJ's own: Resume `F9` (`continue`), Step Over `F8` (`next`), Step
Into `F7` (`stepIn`), Step Out `Shift+F8` (`stepOut`), and Run to Cursor `Alt+F9` — which DAP has
no request for and every client builds the same way: a temporary breakpoint on the caret's line,
`continue`, and removal on the next stop. Each is an `Action` variant in a `DebugAction` sub-enum
(the `RunAction` shape), on the Run menu under the run entries, enabled only while a session is
paused — dimmed, not absent, because "in a moment" is exactly what dimming means here. All go
through `run_action`; nothing reads the keyboard for what is also a menu entry, so the macOS key
equivalents come free and the one-place rule holds.

Function keys while a modal is up belong to the modal (`a_modal_has_the_keyboard` already says
so), and the stepping keys are only alive while a session exists, so `F8` in an editor with no
debugger running means nothing and costs nothing.

### 7.4 The execution point

On `stopped`, Quill fetches the stack, opens the top frame's file if it is not open
(`open_the_match`'s path — re-read at the moment of use), scrolls the least amount that shows the
line (`scroll_to_rect(row, None)`, the explorer-follow rule), and paints the line's band in an
accent-tinted wash behind the text — drawn where `paint_highlights` draws, under the glyphs, in a
colour of its own in `theme::color` so it cannot be mistaken for a person's highlight. Clicking a
different frame moves the point and the variables to that frame without resuming, IntelliJ's
behaviour. The point clears on resume and on session end.

### 7.5 Stopping

The stop button asks `terminate`; a session that has not ended two seconds later, or a second
press, gets `disconnect` and then the child killed — `GRACE`, reused, and the window woken once
when the grace runs out, the run tile's exact arrangement. Closing the window, closing the project
and starting a second session all go through the one stop path.

## 8. Inspection

### 8.1 Frames and threads

The debug tile's left pane is the call stack of the stopped thread: one row per frame — function
name, file name, line — in the list-row furniture (28-point rows, the pill for the selected
frame), with the thread chooser a dropdown above it only when the adapter reports more than one
thread. Frames the adapter marks as `subtle` (library internals) are drawn in the quiet colour,
listed rather than hidden — the comments-and-strings rule from the references list.

### 8.2 The variables tree

The right pane is the selected frame's variables: `scopes` as top-level groups (Locals, Arguments
— whatever the adapter names), each row a name, a type where the adapter gives one, and a value,
with a disclosure triangle wherever `variablesReference` is non-zero. Expansion fetches that
reference's children through the worker — nothing is fetched that is not on screen, the frame-cost
rule applied to a protocol that was designed for it. Expanded paths are remembered by name across
steps, so stepping through a loop does not re-collapse the structure being watched; the *values*
are always refetched because the references died on resume. Rows whose value changed on the last
stop are tinted briefly — IntelliJ's change-marking, which is what stepping is for.

### 8.3 Changing a value

Double-click a value (or `Set Value...` on the row's right-click menu) turns the cell into a field
— `controls::field_text_rect`, the five-fields lesson — and Enter sends `setVariable` with the
row's container reference and name. The adapter answers with the value as the debugger now sees
it, and that answer is what the row shows, not what was typed. Offered only when
`supportsSetVariable`; absent otherwise. A refusal (an invalid expression, a const) is the
adapter's own message, shown at the row.

### 8.4 Watches and Evaluate Expression

Watches are a short list above the variables: expressions kept on the session, each sent through
`evaluate` (context `watch`) at every stop, results rendered as tree rows like any variable — a
watch answering with a structure is expandable, because the answer carries a `variablesReference`
like anything else. Added from a field in the pane and from `Add to Watches` on the editor's text
menu when there is a selection. They are per-person, per-project state: `workspace.conf`, beside
`run.selected`.

`Evaluate Expression` `Alt+F8` is the same request with context `repl`, asked from a small modal —
`components::modal` furniture, a field and a result area, Enter evaluates — seeded with the editor's
selection when there is one. IntelliJ's distinction between the two (persistent versus one-off) is
kept; nothing else about its evaluate dialog (code completion in the expression field, multi-line
code fragments) is attempted in this version.

### 8.5 Inline values

While the program is paused, each visible line that binds a variable in the top frame's scope gets
that variable's value painted after the line's end in the quiet colour — `pos = <10, 4>` — the
way IntelliJ and Zed paint them. DAP has no request for this; it is the client matching names, and
Quill already owns the machinery: `FileSymbols::read` has the file's identifier list sorted by
position, the paused frame's `variables` are already fetched (§4.2), and the painter already takes
a visible-line range. Matching is a walk over the visible lines' identifiers against one HashMap —
nothing that runs once a frame allocates; the match is computed once per stop and cached on the
session, keyed the way `Hover` is keyed. Values that are long are elided; lines that would wrap
are left alone. It is painted decoration, never text in the document.

## 9. The debug tile

The bottom of the window holds one of **three** tiles now: terminal, run, debug. The
`show_the_run_tile` / `show_the_terminal_tile` pair — which exists because leaving the exclusivity
to callers "did not survive first contact" — becomes a trio with the same shape, and every path
that shows any of the three goes through them. The activity bar gains a debug button below the run
tile's (`icon::bug`, drawn: a dot with legs, in the icon file's manner), the View menu gains
`Debug Tile`, and `workspace.conf` gains `debug.visible`.

The tile is the run tile's sibling as the run tile is the terminal's: `terminal_panel::HEADER`,
the same padding, the same splitter with its heights in `settings::Panes`, min and max clamped on
read and on drag. Inside, left-to-right: frames (§8.1), a draggable divider through
`components::splitter`, variables and watches (§8.2), and the header carries the stepping buttons
(drawn icons, each with a plain `widget_info` name — `Resume`, `Step Over`, `Step Into`,
`Step Out`, `Stop Debugging`), the exception-filters flyout (§6.4), and the session's state in
words — `paused at main.rs:14`, `running`, the tint scheme `State::label`/`tint` already uses.
Every control dims when it cannot apply this instant (stepping while running) and the whole tile
is reachable only when a session exists or breakpoints do — a person who has never debugged never
sees it.

The console — the debuggee's terminal — is the run tile (§7.2), not a pane inside the debug tile:
two tiles cannot show at once, so the tile's header ends with a `Console` button that swaps to the
run tile and back, one press, both directions. Stacking both grids was already rejected once, for
the run tile, because two grids take the editing area below the fold; the same sentence answers
the same idea here.

## 10. One action, one place — and the command line

### 10.1 The menu and the keys

`Action::Debug(DebugAction)` — `Start(Option<String>)`, `CurrentFile`, `Resume`, `StepOver`,
`StepInto`, `StepOut`, `RunToCursor`, `Stop`, `ToggleBreakpoint`, `EditBreakpoint`,
`EvaluateExpression`, `ToggleTile` — named in `action_names.rs` as `debug-<verb>` with the
configuration-name-as-argument rule the run family established, dispatched in one `run_action`
arm. The Run menu grows a debug half: `Debug <name>` `Shift+F9`, `Debug Current File`, separator,
the five stepping entries with their keys, `Toggle Breakpoint` `Ctrl+F8`, `Evaluate Expression...`
`Alt+F8`. IntelliJ's keys, kept exactly, because the ticket says mimic IntelliJ and these are the
keys a person's hands know. Entries are enabled by `MenuState` fields (`debug_applies`,
`debug_paused`, `debug_active`) the way `run_active` already gates, and the whole half is absent
when the file's language names no adapter — `Plugins::debugger_for`, asked by the menu, the title
bar and the CLI so the three cannot disagree.

### 10.2 The catalogue

A `debug` area in `quill-cli/src/catalogue.rs`, one row per verb, and therefore — with no further
work — a section the documentation test enforces in `commands.md`, an arm in `app/cli.rs`'s
`cli_debug`, and an MCP tool the day it lands:

```
debug start [name]           start the named (or selected) configuration under its debugger
debug stop                   end the session
debug continue | step-over | step-into | step-out
debug run-to <path> <line>
debug breakpoint add <path> <line>   --condition <expr>  --log <message>
debug breakpoint remove <path> <line>
debug breakpoint list        every breakpoint, with its verified state while a session runs
debug frames                 the paused stack, one frame a line
debug variables [--frame N] [--expand <path>]
debug evaluate <expression...>
debug watch add|remove|list
debug status                 state, configuration, paused location, exit code
```

`debug variables` and `debug frames` answer from the session's already-fetched state and fetch
deeper only for `--expand`, the tile's own laziness. Waiting verbs ride the `Waiting` mechanism
`run output --wait-for` already uses — `debug start --wait-for-pause`, so a script or an agent can
set a breakpoint, start, wait for the stop and read a variable in four commands. That sequence is
the acceptance test of the whole feature, and it is also the feature's second customer: an agent
driving Quill can now observe a program's actual state instead of reasoning about it.

## 11. Where the state lives

- `.quill/breakpoints.conf` — the project's breakpoints (§6.5). Shared, written by the released
  binary only.
- `.quill/workspace.conf` — `debug.visible`, `debug.watches` (per-person, beside `run.selected`).
- `QuillApp` — one `Option<DebugState>` (in `app/debug.rs`, as `app/git.rs` holds `GitState`):
  the `quill_dap::Client`, the session state machine, the fetched stack and variable rows, the
  watch results, the inline-value cache. All of it dies with the session; none of it is written
  anywhere.
- `quill_core::breakpoints` — the open documents' sets, riding the document exactly as highlights
  ride it.
- Settings — `debug.lldb`, `debug.node`: explicit adapter paths, empty meaning "what this machine
  has" (`Settings::shell()`'s sentence).

## 12. Tests

The four layers as the project keeps them:

1. **`quill-dap`, no window, no process.** The codec against byte transcripts, including torn
   frames. The session state machine against scripted adapters — a `Transcript` of
   request-in/messages-out pairs standing where the process would — covering the happy lifecycle,
   an adapter that dies mid-session, `verified: false`, a `stopped` before `configurationDone`,
   and capability gating (a `setVariable` never sent to an adapter that did not offer it).
   `quill_core::breakpoints`: offsets shift under `insert`/`remove_range`, ride the undo snapshot,
   clamp on load — the highlights tests, re-asked.
2. **`quill-app` units.** `breakpoints.conf` round-trips through `store::Values`; a block missing
   `path` is dropped whole. The registry refuses `debug.adapter = gdb` with the house message; the
   older plugins ask for none of what debugging added (the CSS-keys test, re-asked). Every menu
   entry has a name; `debugger_for` and the menu agree about a `.css` file.
3. **Screenshots.** A detached debug session fed fixed DAP JSON — the terminal's fixed-bytes
   precedent — so the pictures are deterministic: the gutter with an enabled, a disabled, an
   unverified and a conditional breakpoint; the debug tile paused, with frames, an expanded
   structure and a watch; the execution point behind a line of source; inline values at line ends;
   the tile-exclusivity picture (debug tile up, run tile up, never both). Accepted images looked
   at before `UPDATE_SNAPSHOTS=1` accepts them.
4. **The real thing.** A fixture program per adapter (a ten-line Rust binary built by the test, a
   `hello.js`), driven end to end through `quill-cli`: breakpoint add, `debug start
   --wait-for-pause`, `debug variables` asserting a real value, `debug step-over`, `debug
   continue`, exit observed — asserted on text, waiting with a timeout, skipped with a message
   naming the missing adapter on a machine without one (a skipped test that says why, never a red
   one that lies about Quill). And the real window: a person sets a breakpoint in a real project,
   steps, and watches a value change — which is the only layer that proves the adapter on this
   machine behaves as the transcripts said.

Performance is measured, not asserted: a `debug_cost` example in the frame-cost manner if the
inline-value pass or the tree redraw ever needs a number — but nothing here runs once a frame
without a revision check, so the budget conversation should never start.

## 13. What is deliberately not here

- **Attach.** Launch only. `attach` is a second lifecycle with its own configuration shape
  (process pickers, ports) and its own ways to fail; the protocol work in §4 is attach-ready, and
  attach is its own ticket.
- **More than one session.** One `Option<DebugState>`. IntelliJ's multi-session tabs cost a
  session chooser in every pane of the tile for a case that is rare in a one-window editor.
- **Debugging a Cargo/npm configuration by deriving the binary** — Zed's locators. Right and
  wanted, and a design of its own (reading Cargo's JSON messages to find the artifact); until
  then the registry's refusal sentence keeps Quill honest (§7.1).
- **Python.** No Python plugin ships, so no `python` registry entry. The day one exists, debugpy
  is the adapter, and it is the easiest of the three.
- **Smart Step Into** (`stepInTargets` — adapter support is patchy), **Force variants**,
  **stepping filters**, **Reset/Drop Frame** (`restartFrame`), **data breakpoints/watchpoints**,
  **memory and disassembly views**, **hot code replace**: each real, each a capability the
  protocol names, none of them what the ticket asked for. The capability gating in §4.2 means any
  of them can arrive later without re-plumbing.
- **Downloading adapters.** Zed fetches them; Quill fetches nothing — the rule that keeps a
  document from making a network request keeps the editor from doing it too. The refusal sentence
  (§5.2) tells a person exactly what to install; `tools/` may grow an install script the way
  `release.ps1` installs `gh`, but the editor itself never reaches out.
- **A DAP server for Quill itself.** Out of scope and out of character; Quill is the client.

## 14. Sources

- https://microsoft.github.io/debug-adapter-protocol/overview — the protocol: framing, lifecycle, breakpoints, the variablesReference model, runInTerminal.
- https://microsoft.github.io/debug-adapter-protocol/specification — the request/response/event reference this design's §4 types come from.
- https://www.jetbrains.com/help/idea/stepping-through-the-program.html — IntelliJ's stepping actions and keys, including the Force variants and filters §13 defers.
- https://blog.jetbrains.com/idea/2023/04/debugger-upskill-variables-evaluate-expression-watches/ — IntelliJ's variables, Set Value, watches and Evaluate Expression behaviour.
- https://zed.dev/blog/debugger — Zed's DAP client: the two-layer architecture, lazy fetching, locators, tree-sitter inline values.
- https://zed.dev/docs/debugger — Zed's shipped adapter list and configuration shape.
- https://github.com/helix-editor/helix/pull/574 — Helix's DAP client, the smallest working precedent.
- https://github.com/vadimcn/codelldb — CodeLLDB, and its wiki's Windows page on MSVC PDB limits.
- https://marketplace.visualstudio.com/items?itemName=llvm-vs-code-extensions.lldb-dap — lldb-dap, the adapter that ships with LLVM.
- https://github.com/microsoft/vscode-js-debug — js-debug and its standalone `dapDebugServer.js`.
- https://github.com/microsoft/debugpy — the Python adapter §13 records for the day a Python plugin exists.
