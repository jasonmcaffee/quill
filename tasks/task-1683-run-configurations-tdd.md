# task-1683 — run configurations: a named command, run from the title bar, watched from a tile

## 1. What was asked

`task-1683`: run configurations, similar to IntelliJ's, so that node servers, Rust programs and
scripts can be started from inside Quill. Clickable from the top right of the title bar. A run panel
and an icon at the bottom left, so a run can be stopped, started again and its output read — the
shape IntelliJ's run widget and Run tool window make together. The ticket also asks a question that
has to be answered before anything is designed: which parts of this belong to plugins — should
running node mean a Node plugin?

This document is the design. `task-1684` is the implementation. Nothing here is built yet.

## 2. What the surveyed editors do

### 2.1 IntelliJ — the behaviour the ticket names

IntelliJ's system is four things that are easy to conflate, and the design below takes a position on
each one separately.

**The configuration model.** A run configuration is a named set of startup properties. Every
configuration is made from a *template* (Application, npm, Cargo, Shell Script — several dozen,
depending on installed plugins), which defines the fields and their defaults. Configurations are
either *permanent* — created deliberately and kept until removed — or *temporary*, created
automatically when you run a file that has no configuration yet. Temporary configurations are drawn
semi-transparent, capped at five with the oldest silently dropped, and can be promoted to permanent
with a save. Configurations can carry *before-launch* tasks (build first, run another configuration
first, run an npm script first), be grouped into folders, and be marked broken with a red cross when
a field no longer resolves.

**Storage.** By default a configuration is written into `.idea/workspace.xml`, which is per-person
and full of unrelated state, so it cannot be shared. Ticking *Store as project file* moves it to its
own XML file under `.idea/runConfigurations/`, which is the shareable form; teams that ignore
`.idea/` wholesale carve that one folder back in via `.gitignore`. Two storage tiers, and the
sharable one had to be retrofitted.

**The run widget.** In the current UI the top right of the main toolbar holds the widget: the
selected configuration's name with a chevron, a green play button, a debug button, and — while
something runs — a stop square and a rerun arrow. The chevron opens a list of the configurations
with the recent ones first, a per-row run action, pinned favourites (2023.2), and
`Edit Configurations…` at the bottom. The widget is deliberately not customisable.

**The Run tool window.** A tool window along the bottom, one tab per running or finished
configuration, reachable from a button in the window's edge rail. Its toolbar: rerun, stop — first
press a soft kill, the SIGINT the program can catch, second press a hard one — pin (a pinned tab is
not reused by the next run), and clear. The console is a real terminal as far as the program can
tell. When the process ends the console prints `Process finished with exit code N` and the tab
stays, so the evidence outlives the process. The *Services* tool window is a second, aggregated view
over the same runs plus databases and containers; it exists because a dozen simultaneous runs as
flat tabs stop being navigable.

### 2.2 WebStorm's Node.js template, and RustRover's Cargo one

The two templates the ticket names, reduced to their fields. Node.js: the node binary, node's own
parameters, the JavaScript file, the application's parameters, a working directory, environment
variables. Cargo: one *Command* line written as `[command] [options] [--] [program args]` — the
whole cargo invocation in a single field — plus a working directory, environment variables,
backtrace, and an *allow multiple instances* tick, off by default, so rerunning stops the previous
run first.

Worth noticing: RustRover already collapsed the form to one command field, because the person
writing `cargo run --release -- --port 3000` knows exactly what they mean and a form that decomposes
it buys boxes, not clarity. The Node template's many fields all compose into
`node [params] file [args]` — one command line wearing six boxes.

### 2.3 VS Code — two files and a seam people fall into

VS Code splits the idea in half: `tasks.json` runs commands (build, lint, a dev server) and
`launch.json` configures the debugger, joined by `preLaunchTask`. Tasks want a *problem matcher* — a
regex that turns output into entries in the Problems panel — and the pestering to pick one is a
documented irritation. The lesson taken: the split exists *because* VS Code has a debugger, and the
problem-matcher machinery is a second feature bolted to the first. An editor without a debugger
needs neither the split nor the matcher.

### 2.4 Zed — the same feature as data

Zed's tasks are the closest existing thing to Quill's temperament: a `tasks.json` of plain data —
label, command, environment, whether to reuse a terminal — spawned *into the integrated terminal*,
with variables like `$ZED_FILE` resolved from editor state, and a rerun that reuses the captured
context. No task types, no plugins-as-code, no separate console: the terminal the editor already has
is the output pane. Sublime Text's build systems are the same tier a generation earlier:
a `.sublime-build` JSON file naming a `cmd`, an optional `file_regex`, and variants.

### 2.5 What was rejected

**A debugger.** IntelliJ's run and debug are one widget, and the ticket says "run configurations",
not "a debugger". A debugger is the Debug Adapter Protocol, per-language adapters found on the
machine, breakpoints in the gutter, a stepping UI — a project several times this one, sitting on
this one. Run only, and the design leaves the seam visible: a configuration is a command, and a
debugger would be a second way of starting the same command.

**Configuration types as code, IntelliJ-style.** IntelliJ's `ConfigurationType` /
`ConfigurationFactory` / `SettingsEditor` exists so that a plugin can ship arbitrary launch logic
with arbitrary UI. Quill's plugins are data and nothing in one is executed; a typed form per
language would mean either widening `plugin.kind` to code — the seam `services::plugins` names and
deliberately leaves empty — or hard-coding a form per language into Quill, which is the same
maintenance surface IntelliJ pays, paid by hand. §2.2 shows the forms collapse to a command line
anyway.

**A second output pane.** A console that is not a terminal re-implements colour, scrollback,
selection and copy, and then a progress bar or a spinner arrives and it is wrong.
`quill_terminal::Session` already runs an arbitrary program in a pseudoterminal —
`SessionSettings { shell, args, working_directory }` — and already receives the child's exit code
(`Event::ChildExit`, currently discarded). The output pane is the terminal stack Quill has.

**Problem matchers.** No Problems panel to feed, and §2.3 is the warning. A later ticket could make
`file:line` in any terminal output clickable, which would serve the shell tabs too and belongs to
the terminal, not to running.

## 3. The shape of the design

One sentence a piece:

- A run configuration is a **named command**: a command line, a working directory, environment
  variables. One kind, not a template per language.
- Running one **spawns a `quill_terminal::Session`** with the program in place of the shell, so
  output is a real terminal and stopping is killing a process Quill owns.
- The **run tile** along the bottom of the window — a sibling of the terminal tile, drawn from the
  same grid — holds one tab per run, with rerun and stop on the tab strip and the exit code written
  where the person is looking.
- The **run widget** sits at the right of the title bar, before the text tools: the chosen
  configuration's name, a play button, a stop square while it runs, and a chevron listing the rest.
- Configurations are **stored in `.quill/run-configurations.conf`**, beside the project state that
  already lives there, in the store format everything else uses.
- Plugins contribute **data, not types**: a manifest may say how a file of its language is run and
  how its projects announce themselves, exactly as `language.renders` names a built-in renderer. No
  Node plugin — node is how the JavaScript plugin's files run.
- Everything is reachable from **`quill-cli run`**, which also makes every part of it an MCP tool an
  agent can call — including reading a dev server's output, which no other editor here offers.

## 4. A configuration is a command

### 4.1 The fields

```
name         Dev server
command      node server.js --port 3000
directory    backend
env          PORT=3000; DEBUG=app:*
```

Four fields. `command` is one line, RustRover-style, split by the same quoting rules a shell uses
for a double-quoted word — the first word is the program, the rest are arguments, and a path with a
space in it is written in quotes. The split is done by Quill and the arguments are passed to the
process as arguments: **no shell runs the command line**, so nothing expands, nothing globs, and a
command containing `&&` is one program with a strange argument rather than two programs. A person
who wants a shell writes `pwsh -Command ...` and has said so in the one place it can be seen.

`directory` is relative to the project root, empty meaning the root itself — the same rule
`.quill/open-files.txt` follows, for the same reason: the project may move. The program is found the
way the terminal's shell is found: an absolute path is used as written, a bare name is looked up on
`PATH`. `env` is `NAME=value` pairs separated by semicolons, RustRover's spelling, laid over the
environment Quill itself started with — never replacing it, because a child with no `PATH` is a bug
nobody enjoys finding.

There is no *type* field. An IntelliJ Node configuration and a Cargo configuration differ in which
boxes compose the command line; written as the command line, they differ in nothing.

### 4.2 Where they live

`.quill/run-configurations.conf`, read by `services::store::Values` like every other file Quill
writes, numbered the way `files.panes` already numbers a list:

```
run.1.name = Dev server
run.1.command = node server.js --port 3000
run.1.directory = backend
run.1.env = PORT=3000; DEBUG=app:*
run.2.name = cargo run
run.2.command = cargo run
```

One file for the whole project, not one per configuration: IntelliJ's one-file-each answers a
merge-conflict problem that XML causes and plain lines mostly do not, and `highlights.txt` already
chose one file for the same reason. It lives in `.quill` because a run configuration belongs to the
*project* — the command that starts this server is a fact about this folder — which is the decision
IntelliJ's *Store as project file* tick lets you make and Quill simply makes. What is per-person
goes where per-person things go: `workspace.conf` gains `run.selected` (the widget's current name)
and `run.visible` (whether the tile is up), beside the terminal flags it already holds.

Only the released binary reads or writes it, exactly as `project_state` is handled, and a file that
cannot be read is a file that is not there. A `run.N` block missing `name` or `command` is dropped
on reading, whole, with the rule the project state keeps: a project that opens with one
configuration missing is better than a project that will not open.

### 4.3 Temporary configurations

*Run current file* (§6.3) runs without asking anything to be filled in, and what it ran appears in
the widget's list as a temporary configuration — drawn in the quiet colour, the way an occurrence
in a comment is listed in the references modal — so it can be rerun and so it can be kept.
Temporary configurations live in memory, at most five with the oldest dropped, and are **not
written to disk**; IntelliJ writes its temporaries into `workspace.xml` and Quill deliberately does
not, because a file the project shares should hold what somebody chose to keep. *Save* in the
dialog, or editing a temporary there, promotes it into `run-configurations.conf`.

## 5. Running one

### 5.1 The session

Starting a configuration builds a `SessionSettings` — program, arguments, resolved working
directory — and calls `Session::spawn`, the same call the terminal tile makes, with two additions
to `quill-terminal`:

- `SessionSettings.env: Vec<(String, String)>` — variables laid over the inherited environment.
  The terminal's shells leave it empty and change by nothing.
- `Session` keeps what `Event::ChildExit(code)` carries instead of discarding it:
  `exit_code: Option<i32>`, readable after `running` goes false.

Both are a few lines in the one file that knows `alacritty_terminal` exists, and both are testable
the way the crate already tests: spawn a real short-lived program, pump, assert the code.

A pseudoterminal rather than captured pipes, deliberately: programs behave differently when their
output is not a terminal — node buffers, cargo drops its colours, progress bars vanish — and the
point of a run panel is to show what the person would have seen. The cost is that a column-perfect
program wants a size; the run tile tells the session its grid size exactly as the terminal tile
does.

### 5.2 One instance per configuration

Pressing run on a configuration that is already running stops it and starts it again — rerun, not a
second copy. That is IntelliJ's default (*allow multiple instances* is a tick almost nobody ticks)
and the honest reading of what a person means by pressing run on `Dev server` twice: the port is
taken; they want the new code. A second *simultaneous* run of the same command is two
configurations with two names, which also gives the two tabs two names.

### 5.3 Stopping

The stop button's first press is polite: the interrupt byte down the pty, `0x03`, which reaches the
program as Ctrl+C — the encoding `quill_terminal::keys` already owns. A program still alive after a
short grace (two seconds) or a second press is killed through the child handle. That is IntelliJ's
soft-then-hard rule, and the grace is short because the person pressing stop twice has already
decided. Closing a run tab, closing the window, or rerunning all take the same path — nothing ever
orphans a child on purpose, and `Session`'s drop already shuts the pty down.

### 5.4 The end of a run

When `running` goes false the tab stays, holding everything the program wrote, with the exit code
written in the tab's strip — `finished`, or `exit code 101` in the error colour when it is not
zero. The grid is never written into by Quill: IntelliJ prints its epilogue into the console, but
that line pretending to be program output is exactly the confusion a separate strip avoids, and the
strip is where the eye already is because rerun and stop live there.

## 6. The two places it is seen

### 6.1 The run tile

A tile along the bottom of the window, the terminal tile's sibling: same header height, same grid
drawing, same padding, same splitter above it, its height its own entry in `settings::Panes`. One
tab per run. The tab shows the configuration's name and a state: a green dot while running, nothing
when it finished cleanly, the exit code in the error colour when it did not. The header's right end
holds three icon buttons for the showing tab — rerun, stop (dimmed once it has stopped: it could
apply again in a moment, which is exactly what dimming means here), and clear — and closing a tab
kills its process by the rule in §5.3.

The bottom of the window shows **either the terminal tile or the run tile**, not both stacked: two
grids stacked take the editing area below the fold of anything, and IntelliJ's bottom tool windows
made the same choice. The activity bar's bottom group gains a second button, `Run tile`, above
`Terminal tile` — named with the same suffix, for the same collision rule that named the first —
and each button shows its own tile, putting the other away. Opening a run brings the run tile up;
`View -> Run Tile` toggles it from the keyboard, beside `View -> Terminal`.

Keyboard and mouse into the grid go to the program exactly as the terminal's do — a run is
interactive, because `node` asking a question deserves an answer — with the terminal panel's
clipboard rules carried over unchanged: something selected and Ctrl+C copies, nothing selected and
it interrupts.

### 6.2 The run widget

At the right end of the title bar, between the project's name and the text tools, inside the height
the title bar already has. Three parts:

- **The name** of the selected configuration, elided past about sixteen characters, with a chevron.
  Clicking it opens the flyout: every configuration — permanent first, temporaries in the quiet
  colour after — each row with a small play icon, a green dot on rows currently running; then
  `Run Current File` when §6.3 offers it; then `Edit Configurations…`. Choosing a row selects it;
  its play icon runs it. One flyout, no submenus, honouring the rule that a flyout must not hold
  another.
- **Play**, running the selected configuration. With none selected it opens the dialog instead,
  which is what `Add Configuration…` means without a second control meaning it.
- **Stop**, drawn only while the selected configuration runs — a control absent when it cannot
  apply, Quill's rule — killing by §5.3.

With no configurations and no runnable file the widget is just the play button that opens the
dialog: present, because the way to discover the feature has to be visible, and small, because it
is not yet in use. The widget never changes the title bar's height and takes nothing from the
window-drag area that `tools_rect` has not already accounted for; it claims its rectangle the same
way the text tools claim theirs.

### 6.3 Run current file

When the open file's language plugin carries `run.file` (§8), the widget's flyout and the `Run`
menu offer `Run Current File`: the manifest's command with `{file}` replaced by the file's path,
run from the project root, creating a temporary configuration (§4.3) named after the file. The
entry is *absent* — not dimmed — for a file whose language names no command, which is the rule
`file_kind` already applies to the three code-navigation entries: `.rs` files never see it, because
running one file of a Cargo project is not a thing cargo does; `server.js` sees `node server.js`.
An unsaved, never-saved document has no path and the entry is absent there too.

## 7. The dialog

`Run Configurations`, built from `components::modal` — the frame, header, footer and buttons every
other modal is built from, with dragging and resizing inherited from `modal::show` for free. Two
columns, the settings window's own shape: the list of configurations on the left with add and
remove under it, the four fields of the chosen one on the right — `Name`, `Command`, `Directory`,
`Environment` — each a `controls` field, each with a plain widget name (`Run configuration name`,
`Run configuration command`, …) so the screenshot tests can find them. A temporary configuration
selected in the list shows the same fields plus a `Save` button that promotes it. The footer is
`Done`. Changes are written to `.quill/run-configurations.conf` when the dialog closes, by the
released binary only.

Removing a configuration whose run is still going stops the run, after the modal's confirmation —
the same furniture the git dialogs use — because silently killing a server somebody is watching is
worse than one extra click. No folders and no before-launch list: §12.

## 8. What a plugin says, and what it never does

The ticket's question: should running node be a Node plugin? **No — node is how JavaScript runs**,
and Quill already has a JavaScript plugin. A separate Node plugin would be a plugin with no
language, no extensions and no tokens, existing to carry one line of data that the JavaScript
manifest can carry itself. The precedent is exact: Mermaid did not widen `plugin.kind` to get its
diagrams drawn; it named a built-in renderer with a data key. Running follows the same seam, with
two keys and a named-detector rule, all off unless a manifest asks:

- **`run.file`** — how one file of this language is run, `{file}` standing for the path:
  `run.file = node {file}` in the JavaScript plugin, `npx tsx {file}` in TypeScript's. Enables
  §6.3 and nothing else.
- **`run.project = <name>`** — names a **project detector built into Quill**, checked against
  `plugins::PROJECT_RUNNERS` exactly as `language.renders` is checked against `RENDERERS`, refused
  by name when this version has no such detector. Two ship at first: `cargo` — when the project
  root holds `Cargo.toml`, offer a `cargo run` configuration — and `npm` — when it holds
  `package.json`, read its `scripts` and offer `npm run <script>` for each. The rust plugin says
  `run.project = cargo`; JavaScript and TypeScript both say `run.project = npm`, and a detector
  named twice runs once.

Detected configurations appear in the widget's flyout under the permanents, in the quiet colour,
as suggestions; running one makes it a temporary (§4.3), saving keeps it. They are suggestions
precisely so that the conf file stays what somebody chose to keep.

**Nothing in a plugin is executed, still.** A manifest contributes a command *line* — data — and
nothing runs until a person presses run, with the command written in front of them in the widget,
the flyout and the dialog. The parsing of `package.json` and `Cargo.toml` is Quill's own code,
shipped in the binary, which is what keeps a third-party manifest from being able to smuggle logic:
the most a manifest can do is suggest text, visibly. And the switch works the way Mermaid's does:
disable the JavaScript plugin and `Run Current File` on `.js` files and the npm suggestions
withdraw in the same frame, because the window asks `Plugins` at the moment of use.

## 9. One action, one place — and the command line

### 9.1 The menu

A `Run` menu between `View` and `Git`, because IntelliJ has one where people will look for it:
`Run <selected name>` — the name live in the entry, as `Git` entries already name the branch —
then `Run Current File`, `Stop <name>` (dimmed when nothing runs), `Rerun`, a separator, the
configurations each as an entry, a separator, `Edit Configurations…`. Every entry is an
`app::actions::Action` variant dispatched in `run_action`, so the widget, the menu, the rail and
the keyboard cannot disagree. Key equivalents follow IntelliJ per platform — `Shift+F10` run and
`Ctrl+F2` stop on Windows, `Ctrl+R` and `Cmd+F2` on macOS — and the existing macOS
one-key-one-item test polices the additions.

### 9.2 The catalogue

A new area `run` in `quill-cli/src/catalogue.rs`, one arm each in `app/cli.rs`:

```
run list                    the configurations, with state and exit codes, as JSON
run add <name> <command> [--directory d] [--env pairs]
run remove <name>
run start [name]            the selected one when no name is given
run stop [name]
run rerun [name]
run select <name>
run output [name] [--tail n]   what the run has written, as text
run status [name]           running or not, and the exit code
```

`run output` is the one to notice: it reads the run's `Screen` — the same screen the painter reads —
so an agent can start a dev server, read its port from the log, exercise it and stop it, without a
person watching. The catalogue is what the MCP server serves, so every one of these is a tool the
day it lands, documented by the documentation test or failing it.

## 10. Where the state lives

`services::run_configurations` owns the model: reading and writing the conf file, the temporaries,
the command-line split, the env parse, the detectors' output — all pure, all unit-testable with a
temporary folder and no window. `QuillApp` holds a `RunPanel` the way it holds the
`TerminalPanel`: visibility, focus, and a `Vec` of runs, each a configuration snapshot plus its
`Session`. A run holds the *snapshot*, not the configuration's name, so editing a configuration
mid-run changes what the next run does and never what the tab says about the one that already
happened. `components::run_panel` and the widget in `components::title_bar`'s rectangle draw what
those hold and report what was pressed, deciding nothing, as every component does.

## 11. Tests

The four layers as the project keeps them:

1. **`quill-terminal`**: `SessionSettings.env` reaches the child; a spawned `cmd /c exit 3` (and
   `sh -c 'exit 3'` on macOS) ends with `running` false and `exit_code == Some(3)`.
2. **`quill-app` units**: the conf file round-trips, a block missing its command is dropped whole,
   the command splitter handles quotes and refuses nothing, env pairs parse, relative directories
   resolve, detectors read a real `package.json`/`Cargo.toml` from a temporary folder, temporaries
   cap at five, and the plugins that shipped before ask for none of the new keys.
3. **Screenshots**: the run tile drawn from a detached session fed fixed bytes — the terminal's own
   trick, so no real program and no timing; the widget idle, running, and stopped-with-error; the
   flyout with permanents, temporaries and suggestions; the dialog. Real-process tests assert on
   text with a timeout and pump, never `Harness::run`.
4. **The real window**: run a node server from the widget, watch it in the tile, stop it; then the
   same three from `quill-cli run` — which also exercises `run output` for the agent case.

`action_names.rs` fails if any new menu entry lacks a name, and the documentation test fails until
`commands.md` says what `run` does. No performance budget is added: nothing here runs once a frame
beyond what the terminal tile already proves cheap, and the session pumps on the waker it already
has.

## 12. What is deliberately not here

- **A debugger** (§2.5). The seam it would need — a configuration is a command with a name — is the
  seam this builds.
- **Before-launch tasks and compound configurations.** Cargo builds before it runs and npm scripts
  compose in `package.json`; the machinery earns its keep the day somebody actually chains two
  Quill configurations, and a `run.N.before` key slots into the format without migration.
- **Problem matchers, and clickable `file:line` in output.** The second is worth a ticket of its
  own and belongs to the terminal stack, where the shell tabs get it too.
- **A Services window.** Aggregation pays at a dozen simultaneous runs; the tile's tabs are that
  window at Quill's size.
- **Allow-multiple-instances.** Two names make two runs (§5.2).
- **Folders, favourites and pinned tabs** — organisation for a hundred configurations a `.quill`
  project does not have.

## 13. Sources

- jetbrains.com/help/idea/run-debug-configuration.html — the model: templates, temporary versus
  permanent and the cap of five, storage and *Store as project file*, folders, the widget.
- jetbrains.com/help/idea/run-tool-window.html — the tool window: tab per run, rerun, soft-then-hard
  stop, pin, the console toolbar.
- plugins.jetbrains.com/docs/intellij/run-configurations.html — the SDK seam:
  `ConfigurationType`, `ConfigurationFactory`, `RunConfiguration`, `SettingsEditor`.
- jetbrains.com/help/webstorm/run-debug-configuration-node-js.html — the Node.js template's fields.
- jetbrains.com/help/rust/cargo-run-debug-configuration.html — Cargo's one command field, *allow
  multiple instances*, environment.
- blog.jetbrains.com/idea/2023/03/new-ui-enhancements-in-intellij-idea-2023-1 — the redesigned run
  widget; 2023.2 notes for pinning.
- code.visualstudio.com/docs/debugtest/tasks and microsoft/vscode#62728 — tasks versus launch, and
  the problem-matcher irritation.
- zed.dev/docs/tasks — tasks as data, spawned into the terminal, context variables, rerun.
