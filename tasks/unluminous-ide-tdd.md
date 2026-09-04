# Unluminous as a code editor: line numbers, tabs, git and plugins

A technical design for the six things `task-1649` asks for. It records what was chosen, what was
considered and rejected, and where each piece belongs, so that the work reads like the rest of Unluminous
rather than like a second editor bolted to the side of it.

Unluminous today is a good editor for prose. It opens any file holding text, so a `.rs` or a `.ts` file
already opens — as one long stretch of undifferentiated text with no line numbers, one file at a
time, with no idea that the folder it is looking at is under version control. This closes that gap.

The six pieces, in the order they are built, because each one leans on the one before it:

1. A **style guide**, so every new control is judged against something written down.
2. **Line numbers**, and the strip beside them that annotates with git blame.
3. **File tabs**, so more than one file can be open.
4. The **explorer's right click menu**: new file, cut, copy, paste, rename, reveal, reload.
5. **Git**, in full: commit, push, pull, fetch, merge, rebase, rollback, branches, history, stash.
6. A **plugin architecture** with a marketplace inside the application, and three plugins —
   JavaScript, TypeScript and Rust — that give those files an icon, syntax colouring and a theme.

---

## 0. What this keeps to

Everything in `CLAUDE.md` still holds, and three of its rules decide most of what follows.

**One action, one place.** Every new thing a menu or a shortcut can ask for is an
`app::actions::Action` with an arm in `UnluminousApp::run_action`. Git has twenty-odd entries and the
explorer's context menu has ten; both go down that one path, so the menu bar, the context menu, the
macOS bar and a test cannot disagree about what `Commit` means.

**Components take a rectangle and return what happened.** The gutter, the tab strip, the commit panel
and the plugins page are all functions of `(Ui, Rect) -> Outcome`. None of them writes to a
`Document`, starts a git command or installs a plugin.

**Nothing in `unluminous-core` knows about a window.** Syntax highlighting produces spans over text, which
is exactly the shape the Markdown parser already produces, so it belongs there and its tests run with
no graphics card. Git is neither the editor nor the terminal, so it gets a crate of its own on the
same terms: no user interface dependency, tests that run headless.

### The crates after this work

| Crate | What is in it | What must never be in it |
|---|---|---|
| `unluminous-core` | The editor: buffer, formatting, caret, layout, undo, the Markdown parser, **and the syntax tokeniser**. | Any user interface dependency. |
| `unluminous-terminal` | The terminal. | Any user interface dependency. |
| `unluminous-git` | **New.** Reading and changing a git repository. | Any user interface dependency, and any decision about what a dialog looks like. |
| `unluminous-app` | The window: drawing, input, fonts, settings, menus, **the plugin registry**. | Editor behaviour, terminal emulation, or git plumbing. |

`unluminous-git` is a crate rather than an `unluminous-app/services/git.rs` for one reason that matters: its
tests make real repositories in a temporary folder, run real commands against them and assert on the
results, and they must be able to do that with no window, no graphics card and no fonts — the same
bar `unluminous-core` is held to. A module inside `unluminous-app` would drag `eframe` into every one of those
runs.

---

## 1. The style guide

`design/style-guide.md`, next to the images it refers to.

Unluminous already has a palette read out of `design/intial-design-screenshot.png` rather than chosen by
eye, and a set of measurements taken from the same image. What it has not had is a written statement
of *how a new control is built out of them*, which is what an agent adding a context menu or a modal
needs. Six new components arrive in this work; without a guide they would each invent their own row
height.

The guide states, and every new component in this design obeys:

- **The palette is closed.** `theme::color` is the whole list. A new colour is added there, with the
  region of the design it was read from, or it is not used. The one exception is the syntax theme,
  which is a plugin's own palette and is discussed in section 6.
- **The measurements are closed** in the same way: `theme::size`, plus the per-component constants
  named in this document.
- **A row is 28 points high** — `size::ROW` — in the explorer, the settings list, and now the git
  changes tree and the plugin list. A menu row is 24. Nothing invents a third height.
- **Selection is a filled pill** at `color::SELECTED_ROW`, inset 8 points horizontally and 1
  vertically, corner radius 5. Hover is the same pill at `color::CONTROL`. That is how the open file,
  the chosen settings page, the chosen plugin and the chosen commit are all drawn.
- **A modal** is `egui::Modal` with a backdrop of `from_black_alpha(120)`, filled `color::EXPLORER`,
  a one point `color::CONTROL_BORDER` stroke and a corner radius of 10. It has a 46 point header
  filled `color::TITLE_BAR` with the title at the left and a close cross at the right, and a 52 point
  footer with its buttons at the right. The settings window is the reference; the commit panel and
  every git dialog are built the same way.
- **Every control is named** with `response.widget_info`, in plain words, and no two controls in one
  window share a name. This is not decoration: it is the only way the screenshot tests find anything.
- **Icons are drawn, not lettered.** `theme::icon` gains one function per new icon. egui's default
  fonts have no glyph for most symbols and an absent glyph renders as an empty box, which is how the
  existing icons came to be drawn in the first place.
- **A pane is resized through `components::splitter`**, added to the `Ui` after the panes either side
  of it.

It also names the baselines, which is the part `task-1649` asks for directly: `design/` holds the
images a new component is compared against, and a component that has no baseline gets one before it
is called finished. The existing two are joined by one image per new piece, captured from the real
window and stored under `design/components/`.

---

## 2. Line numbers, and the blame strip

### What it looks like

A gutter down the left of the editing area, inside `editor_rect`, before the text:

```
| blame (only when annotating) | numbers | gap | text
```

The **numbers** are right aligned in a column exactly wide enough for the largest line number in the
file, plus 8 points either side, so the column does not twitch as the file grows past 99 lines. They
are drawn in `color::TEXT_FAINT`, except the line the caret is on, which is `color::TEXT_CONTROL`, as
IntelliJ does it.

The **gap** is the 12 point strip to the right of the numbers that `task-1649` names. Right clicking
anywhere in the gutter — the numbers or the gap — opens a menu holding `Annotate with Git Blame` and
`Show Line Numbers`. The gap is where a folding arrow would go if Unluminous ever grows folding; leaving
it empty now is what makes room for it later without moving the text.

The **blame column** appears at the far left when annotation is on, and shows `date  author` a line,
as in the reference capture. Its background is tinted by how old the commit is: the newest commit in
the file is drawn at `#B4588C`, the oldest at `#3C7D64`, and everything between is interpolated by
rank rather than by date, so a file whose history is one recent burst and one ancient commit still
reads as a gradient. Both colours were measured out of `tasks/assets/1787624688880-paste.png` rather
than picked, in the same way the rest of the palette was measured out of the design.

Hovering a blame row shows the full commit: hash, author, date and subject. Clicking one opens that
commit in the history window (section 5).

### Numbers count paragraphs, not rows on screen

Unluminous wraps. One paragraph is several `PlacedLine`s, and a numbered wrapped line would count the
wrapping, which is not what a line number means. `PlacedLine` already carries `paragraph`, so the
gutter draws a number on a visual line only when its `paragraph` differs from the line above it, and
the number is `paragraph + 1`. A wrapped paragraph therefore shows one number against its first row
and nothing against its continuations, which is what every editor does.

This is the whole reason the gutter is drawn from the `Layout` rather than from the text: it has to
line up with rows on screen that the text alone cannot predict.

### Where it lives

`components/gutter.rs`, a component like any other:

```rust
pub struct GutterOutcome {
    /// The gutter was right clicked, at this position, on this line.
    pub context_menu: Option<(Pos2, usize)>,
    /// A blame row was clicked, so the window should show that commit.
    pub show_commit: Option<String>,
}

pub fn show(ui: &mut Ui, area: Rect, state: &GutterState, layout: &Layout, scroll: f32,
            caret_line: usize) -> GutterOutcome
```

`GutterState` holds whether numbers are showing, whether annotation is on, and the blame for the open
file when it is. It is per document, so annotating one tab does not annotate the others.

The editing area shrinks by the gutter's width, and the padding after the gutter is the small one
(`editor_view::PADDING`) rather than the wide one the pane's own left edge gets. With the gutter put
away the text keeps exactly the padding it always had, so hiding the numbers leaves the window
looking as it did before there were any.

### Settings

`Editor -> General -> Show line numbers`, on by default for every file. There is no "code files only"
rule, because Unluminous has no notion of a code file that is worth a setting: a `.txt` file with numbers
down the side is not wrong.

---

## 3. File tabs

### What it looks like

A 32 point strip along the top of the editing area — not the whole window, because the explorer is to
the left of it and the tabs belong to the editor, which is where IntelliJ puts them. Each tab holds
the plugin's file icon (or the existing coloured square when no plugin claims the file), the file
name, and a close cross. The active tab is filled `color::SELECTED_ROW` and carries a **two point
`color::ACCENT` line along its bottom edge**, which is the underline `task-1649` asks for. A tab with
unsaved changes shows the amber dot Unluminous already uses for that, in place of the cross until the
pointer is over it.

When the tabs do not fit, the strip scrolls sideways rather than shrinking them to unreadable stubs.

### One click and two clicks

IntelliJ's behaviour, which is what the ticket describes: **a single click on a file in the explorer
opens it in the transient tab, and a double click opens it in a tab of its own.**

There is one transient tab at most. Single clicking a second file replaces its contents rather than
adding a tab, so browsing a folder does not leave thirty tabs behind. Its name is drawn in italic to
say so. Double clicking the transient tab's file, or editing it, makes it permanent — editing a file
you were only glancing at plainly means you meant to open it.

### What a tab is

Everything that is per document moves out of `UnluminousApp` and into the tab:

```rust
pub struct OpenFile {
    pub document: Document,
    pub view_mode: ViewMode,
    pub scroll: f32,
    pub preview_scroll: f32,
    pub gutter: GutterState,
    /// True while this is the tab a single click reuses.
    pub transient: bool,
}

pub struct OpenFiles {
    files: Vec<OpenFile>,
    active: usize,
}
```

`UnluminousApp::document` becomes `UnluminousApp::files.active().document`. The laid out `Layout` stays on
`UnluminousApp` as a cache of the active file only, and switching tabs sets `layout_stale`, exactly as
opening a file already does — the revision counter starts again for each document, so comparing
revisions across a switch would keep the wrong layout. That was already a real fault once, recorded
in `app/mod.rs`, and switching tabs is the same fault wearing a different hat.

An empty window still has one tab, holding the untitled document, so there is never a state with no
document and a special case for it.

### Shortcuts

`Close Tab` is **Ctrl+F4** on Windows and **Cmd+W** would have been the obvious choice on macOS —
except `Cmd+W` is already `Close Window` and one key equivalent claimed by two menu items is a fault
on macOS, which `actions.rs` already has a test for. So `Close Tab` is Ctrl+F4 on both, and Close
Window keeps Cmd+W. `Alt+Right` and `Alt+Left` move to the next and previous tab, which is what
IntelliJ uses. Middle clicking a tab closes it.

---

## 4. The explorer's right click menu

Right clicking a row in the explorer opens a menu at the pointer. It holds, in the order the
reference capture shows them:

| Entry | What it does |
|---|---|
| `New > File` | Asks for a name and creates an empty file in this folder, or in the folder holding this file. Any extension. |
| `Cut` | Puts the path on Unluminous's file clipboard, marked to move. |
| `Copy` | Puts the path on Unluminous's file clipboard, marked to copy. |
| `Copy Path` | Puts the path's text on the system clipboard. |
| `Paste` | Copies or moves what is on the file clipboard into this folder. |
| `Rename…` | Asks for a new name and renames the file or folder. |
| `Open in Explorer` / `Reveal in Finder` | Opens the platform's file manager with the entry selected. |
| `Reload from Disk` | Reads the folder again, and re-reads the file into its tab if it is open. |
| `Git >` | The submenu described in section 5. |

**Delete is deliberately absent.** The reference capture has one and `task-1649` does not ask for
one. A destructive entry nobody asked for, one row below `Rename…`, is worth leaving out until it is
wanted; `Cut` and `Paste` already cover moving something out of the way.

### The pieces this needs

**A menu at a position.** `components::menu_bar` already renders a `Vec<Entry>` into a popup, and
that rendering moves into `controls::menu_rows` so the bar and the context menu share it. The context
menu is `egui::Popup::from_position`, framed the same way the bar's menus are. One renderer means the
tick, the dimming, the shortcut column and the row height cannot drift between the two.

**A file clipboard.** `services::file_clipboard`: a path and whether it was cut or copied. Not the
system clipboard, because the system's file clipboard is a different platform interface on each
platform (`CF_HDROP` on Windows, `NSFilenamesPboardType` on macOS) and `arboard`, which Unluminous already
depends on, exposes neither. Copying a path *as text* does go to the system clipboard, because that
is just text. The consequence is stated plainly in the guide: you cannot cut a file in Unluminous and
paste it in Explorer. That is a fair trade against a platform-specific dependency for each platform.

**A prompt.** `components::prompt_dialog`: a modal with a title, a line of explanation, one text
field and `OK`/`Cancel`. New File, Rename, New Branch, New Tag, Stash and Clone all use it, so there
is one text prompt in Unluminous rather than six.

**A confirmation.** `components::confirm_dialog`, the same shape with two buttons and no field, used
by anything that cannot be undone — Rollback, Reset HEAD hard, Drop Stash.

**Reveal in the file manager.** `explorer` on Windows with `/select,`, `open -R` on macOS. Two lines
in `services::launcher`, which already knows how to start a process.

---

## 5. Git

### Why the `git` binary and not a library

The alternatives were `git2` (libgit2 bindings) and `gix` (gitoxide, pure Rust). Both were rejected.

The decisive argument is not size or build time, it is **what a user's git already knows**. A person
running Unluminous has a git that has been configured: a credential helper holding their token, an ssh
agent, `commit.gpgsign`, `core.autocrlf`, hooks, `include` directives, per-repository identity,
`safe.directory`. `git push` from Unluminous must be the same push they get from their terminal, or the
first time it fails they have two gits to debug instead of one. libgit2 reimplements enough of that
to be *nearly* the same, which is worse than being plainly the same thing.

Against it: the output has to be parsed, and a parser is a place for bugs. That is answered by
choosing the formats git provides *for* being parsed — `--porcelain=v2 -z` for status,
`--line-porcelain` for blame, and `--format` with unit and record separators for log — none of which
change between git versions, all of which are NUL or control-character delimited so a path with a
space or a newline in it does not break them.

`git` is looked for once at startup. If it is not there, every git entry is dimmed and the status bar
says why, rather than each operation failing separately with a confusing message.

### `unluminous-git`

```
crates/unluminous-git/src/
  lib.rs        Repository::discover, and the error type
  command.rs    running git and capturing stdout, stderr and the exit status
  status.rs     porcelain v2 -> Status { branch, upstream, ahead, behind, entries }
  blame.rs      line-porcelain -> Blame { lines: Vec<BlameLine> }
  log.rs        formatted log -> Vec<Commit>
  diff.rs       unified diff for a path, against the index, HEAD or a revision
  branch.rs     listing, creating, checking out, deleting, merging, rebasing
  ops.rs        add, restore, commit, push, pull, fetch, reset, stash, tag, remote, clone
  worker.rs     a thread that runs one request at a time and posts the reply back
```

Every call returns `Outcome { ok: bool, stdout: String, stderr: String }` even when it fails, and the
window shows **git's own message** when something goes wrong. A merge conflict, a rejected push, a
detached HEAD and a missing upstream all have good messages already; inventing worse ones would be a
step backwards.

### Nothing blocks the window

`git fetch` over a slow link takes seconds and `git status` on a large repository is not free either.
Every operation runs on a worker thread:

```rust
pub enum Request { Status, Blame(PathBuf), Log { limit: usize }, Commit { .. }, Push { .. }, ... }
pub enum Reply   { Status(Status), Blame(PathBuf, Blame), Log(Vec<Commit>), Done(String, Outcome), ... }
```

`Worker::send` queues a request; `Worker::poll` drains replies each frame. The worker holds the same
`Waker` the terminal uses — a function that calls `Context::request_repaint` — so a reply arriving
while the window is idle draws it immediately rather than waiting for the next mouse move.

While a request is in flight the status bar shows what is running, and the entry that started it is
dimmed, so `Push` cannot be pressed twice.

Status is refreshed after any operation that could change it, and when the window regains focus.
There is no timer polling the repository: a poll that runs whether or not anything happened is how an
editor comes to use processor time while sitting still.

### The Git menu

A fifth menu in the bar, after `View`, holding what the reference capture holds:

```
Commit…                Ctrl+K
Add                    Ctrl+Alt+A
------
Show Diff              Ctrl+D
Compare with Revision…
Compare with Branch or Tag…
Show History
------
Rollback…              Ctrl+Alt+Z
------
Push…                  Ctrl+Shift+K
Pull…
Fetch
------
Merge…
Rebase…
------
Branches…              Ctrl+Shift+`
New Branch…
New Tag…
Reset HEAD…
------
Stash Changes…
Unstash Changes…
------
Manage Remotes…
Clone…
```

The same list, minus the entries that need no file, is the `Git >` submenu on an explorer row, built
from the same function so the two cannot drift.

`Ctrl+Shift+`` collides with nothing: the terminal is `Ctrl+`` with no shift. The existing test that
asserts every shortcut in the bar is claimed by exactly one entry covers the whole new menu for free.

### The commit panel

A modal laid out like the reference capture, which is IntelliJ's commit tool window:

- A **`Commit` / `Shelf` tab strip** at the top. Shelf shows the stash list, because a stash is what
  Unluminous has and a shelf is IntelliJ's own thing; the tab is named for what it does, `Stashes`.
- A **changes tree**: a repository row carrying the branch name in a chip, then one row per changed
  file, each with a checkbox, the plugin's file icon, the file name, and its folder dimmed after it.
  A second group, **`Unversioned Files`**, holds files git does not track yet, collapsed by default
  and ticking it stages them — which is exactly what the capture shows and what
  `git add` of an untracked path does.
- **`Amend`**, which turns the commit into `git commit --amend` and loads the previous message into
  the box.
- The **counts** at the right: `N added   N modified`.
- The **message box**, and a clock button offering the last twenty messages from `git log`.
- **`COMMIT`** and **`COMMIT AND PUSH…`**.

Ticking a file stages it and unticking unstages it, immediately, through `git add` and
`git restore --staged`. The alternative — remembering ticks in the window and staging everything at
the moment of commit — means Unluminous's idea of what is staged and git's disagree for as long as the
panel is open, and the person who runs `git status` in Unluminous's own terminal while the panel is open
sees the disagreement. Staging as you tick keeps one truth.

Selecting a row shows its diff underneath, read only, coloured with the syntax theme's added and
removed colours.

### What each operation runs

Recorded here because "Rollback" and "Reset" are the two that are most often assumed wrongly:

| Entry | Command | Note |
|---|---|---|
| Add | `git add -- <paths>` | |
| Commit | `git commit -m <message>` (`--amend` when ticked) | Hooks run. |
| Commit and Push | the above, then the push dialog | |
| Rollback | `git restore --source=HEAD --staged --worktree -- <paths>` | **Discards** uncommitted changes to those files. Confirmed first, and the confirmation says the changes cannot be recovered. |
| Push | `git push` with the chosen remote and branch, `--force-with-lease` when forced | Never bare `--force`. |
| Pull | `git pull` with the chosen strategy (merge or rebase) | |
| Fetch | `git fetch --all --prune` | |
| Merge | `git merge <branch>` with optional `--no-ff`, `--squash` | |
| Rebase | `git rebase <branch>`, and `--continue` / `--abort` when one is in progress | |
| Reset HEAD | `git reset --soft\|--mixed\|--hard\|--keep <revision>` | Four modes, each explained in the dialog in one line. Hard is confirmed. |
| Branches | `git branch`, `git switch -c`, `git switch`, `git branch -d/-D` | |
| New Tag | `git tag <name>` | |
| Stash | `git stash push -m <message> [--include-untracked]` | |
| Unstash | `git stash apply` or `git stash pop` on the chosen entry | |
| Show History | `git log --format=… -n 200` for the file or the repository | |
| Show Diff | `git diff` / `git diff --cached` / `git diff <rev>` | |
| Compare with Revision | `git show <rev>:<path>` against the working copy | |
| Manage Remotes | `git remote -v`, `add`, `remove`, `set-url` | |
| Clone | `git clone <url> <folder>`, then open it in a new window | |

A conflicted merge or rebase is not hidden: the status bar says `Merging` or `Rebasing`, the conflicted
files are marked in the explorer, and the Git menu grows `Continue` and `Abort`. Unluminous does not have a
three way merge editor, and this design does not add one — the conflicted file opens with its markers
in it, which is a file holding text and therefore something Unluminous already edits.

### Where git shows in the rest of the window

- The **status bar** shows the branch, and how far ahead or behind its upstream it is.
- The **explorer** tints a row by its status: modified in `color::ACCENT`, added in the theme's green,
  untracked in `color::TEXT_FAINT`, ignored dimmed further.
- The **gutter** shows a change bar: a two point stripe against each line changed since HEAD, in the
  same colours. It falls out of the diff the panel already fetches, and it is the single most useful
  thing an editor's gutter does.

### The test repository

Git cannot be verified by looking at a screenshot. `crates/unluminous-git/tests/` builds real repositories
in a temporary folder — `git init`, a first commit, a branch, a conflict, a remote that is another
folder on disk — and runs every operation against them, asserting on `git status` afterwards rather
than on Unluminous's own idea of what happened. A second repository is created under `sample/git-demo/`
for driving the window by hand, with a handful of commits by different authors on different dates so
that blame has something to colour.

---

## 6. Plugins

### What a plugin is

`task-1649` asks for plugins that give a file type an icon, identify its keywords, function names,
imports and comments, and supply a theme. That is a **description of a language**, not a program.

So a plugin is **data**, not code: a folder holding a manifest, an icon, and the words that make up a
language. Loading one is reading a file. Nothing is executed.

```
javascript/
  plugin.conf      the manifest: name, version, extensions, keywords, rules, theme
  icon.png         32x32 with an alpha channel
```

`plugin.conf` is the same `name = value` format `settings.conf` already uses, read by the same
`services::store::Values`. No new dependency, and a plugin can be read and corrected in a text
editor — which is fitting, in a text editor.

```conf
# Unluminous plugin.
plugin.id           = javascript
plugin.name         = JavaScript
plugin.version      = 1.0.0
plugin.vendor       = Unluminous
plugin.description  = Syntax colouring, a file icon and a colour scheme for JavaScript.

language.extensions = js, mjs, cjs, jsx
language.keywords   = const, let, var, function, class, extends, return, if, else, for, while, ...
language.builtins   = console, window, document, Promise, Array, Object, JSON, Math
language.line_comment  = //
language.block_comment = /*, */
language.strings    = ", ', `
language.numbers    = true

theme.name          = Dracula
theme.keyword       = #FF79C6
theme.function      = #50FA7B
...
```

### Why data and not a dynamic library or WebAssembly

Both were considered, and both are the right answer to a question this is not asking yet.

A **dynamic library** (`cdylib`, loaded with `libloading`) would let a plugin run arbitrary Rust. It
also means an unstable ABI between the plugin and the host — a Rust `struct` passed across a
`dlopen` boundary is undefined behaviour unless both sides were built by the same compiler with the
same flags — so every plugin would have to be rebuilt for every Unluminous release, and a plugin crash
takes the editor with it. For "colour these keywords" that is an enormous amount of risk bought for
nothing.

**WebAssembly** solves the safety and the ABI, and costs a runtime — `wasmtime` is a large
dependency, and it brings a host interface that has to be designed, versioned and documented before
the first plugin can be written. It is the right answer the day a plugin wants to *do* something: run
a formatter, talk to a language server, add a tool window.

The extension point is therefore named now and left empty: `PluginKind` in the manifest is `language`
today, and `plugin.kind` is read and checked, so a manifest saying `kind = wasm` is refused with a
clear message rather than silently half-loaded. That is the seam a later version widens.

### The tokeniser

`unluminous-core/src/syntax.rs`, next to the Markdown parser and for the same reason: it reads text and
produces spans, it draws nothing, and its tests run headless.

```rust
pub enum Token { Keyword, Builtin, Function, Type, String, Number, Comment, Operator, Punctuation, Text }

pub struct Grammar { /* the words and rules out of a manifest */ }

/// One linear pass. Comments and strings win over everything, because a keyword inside a
/// string is not a keyword.
pub fn highlight(text: &str, grammar: &Grammar) -> Vec<(Range<usize>, Token)>
```

One pass, no regular expressions, no dependency. The rules, in the order they are tried: a line
comment runs to the end of the line; a block comment runs to its terminator; a string runs to its
matching quote, respecting backslash escapes; a number is a run of digits with an optional decimal
point, hex prefix or suffix; a word is a keyword or a builtin if it is in the list; a word directly
followed by `(` is a function; a word starting with a capital letter is a type; anything else is
text.

That last pair is a heuristic and is written down as one. `Promise.all(` colours `all` as a function
and `Promise` as a type without Unluminous understanding a single thing about JavaScript, which is what
the ticket asks for and is what a colouring pass is for. Real understanding is a language server,
and a language server is not in this design.

**What this deliberately does not do**: nested block comments in Rust (it treats the first `*/` as
the end), template literal interpolation, JSX as anything other than text, and regular expression
literals, which cannot be told from division without parsing. Each is listed in the plugin's own
description so nobody has to discover it.

### Applying the colours

Highlighting must not be an edit. It pushes nothing onto the undo history and does not mark the file
as modified, for exactly the reasons `Document::set_base_style` gives about the font: what Unluminous
saves is plain text and carries no formatting, so nothing about the file has changed.

`Document::set_syntax(spans)` is added beside `set_base_style` and follows the same three rules: no
undo entry, no modified flag, and the revision bumped so the text is laid out again.

It runs when the document's revision changes, and it costs one linear pass over the file. Above
**2 MB** it is switched off and the status bar says so, because a pass on every keystroke of a file
that large is a pause a person can feel. That number is a measurement to be taken, not a guess to be
kept: the limit is a constant with a comment saying what it was measured at.

### The theme

A theme is a map from `Token` to a colour, and the default is **Dracula**, because that is what
`task-1649`'s reference capture is. Not by eye — the capture was sampled, and 643,069 of its 773,520
pixels are within 10 of `#282A36`, with `#FF79C6`, `#8BE9FD`, `#50FA7B`, `#FFB86C` and `#F1FA8C` all
present in the glyphs. The sampling script is kept in `_agent_output/`, in the same spirit as
`examples/sample_design.rs`.

**A theme colours the tokens and not the background.** Dracula's `#282A36` is not used, and Unluminous's
own `color::EDITOR` stays. The window letting the desktop show through is the product's whole
character, and a colour scheme that repaints the editing area opaque would take that away in exchange
for being a shade nearer the screenshot. `Settings -> Editor -> Colour Scheme` offers `Dracula` and
`Unluminous`, the second built from Unluminous's own palette for anyone who wants the two to match.

### The registry

`unluminous-app/src/services/plugins.rs`.

```rust
pub struct Plugin { pub id, pub name, pub version, pub vendor, pub description,
                    pub extensions: Vec<String>, pub grammar: Grammar,
                    pub theme: SyntaxTheme, pub icon: Option<Vec<u8>>, pub enabled: bool,
                    pub bundled: bool }

pub struct Plugins { installed: Vec<Plugin> }
impl Plugins {
    pub fn load(store: &Store) -> Self;               // bundled, then %APPDATA%\Unluminous\plugins
    pub fn for_path(&self, path: &Path) -> Option<&Plugin>;
    pub fn install(&mut self, store: &Store, id: &str) -> io::Result<()>;
    pub fn set_enabled(&mut self, store: &Store, id: &str, on: bool);
}
```

The three plugins are **bundled into the binary** with `include_str!` and `include_bytes!`, so a
fresh Unluminous has a marketplace with something in it and colours a `.rs` file the first time it opens
one. Pressing `Install` writes the folder to `%APPDATA%\Unluminous\plugins\<id>\` and reloads it **from
disk**, which is what proves the loader works on real files rather than only on baked-in data. A
plugin on disk shadows the bundled one of the same id, so a bundled plugin can be corrected by hand.

Which plugins are switched off is remembered in `settings.conf` as `plugins.disabled = a, b`.

A plugin that will not parse is skipped, with its name and the reason in the status bar. Unluminous
starting with one plugin fewer is better than Unluminous refusing to start — the same rule `store.rs`
already keeps for a corrupt settings file.

### The marketplace

`Settings -> Plugins`, laid out like the reference capture and built out of the settings window's own
parts, because a second window that looks nearly the same is worse than one that looks the same:

- A search box, and `Marketplace` / `Installed` tabs with a count on `Installed`.
- The list: icon, name, version and vendor, and a tick on the ones that are enabled.
- The detail pane at the right: the plugin's name, its vendor, a `DISABLE`/`ENABLE` button and a
  version, `Overview` / `What's New` / `Additional Info` tabs, and the description.

The catalogue is the bundled set. There is no network call, and there is not going to be one in this
version: fetching a plugin over the network means signature checking, a trust decision and a
downloaded-code story, none of which a data-only plugin format has earned yet. The list is honest
about it — the marketplace says these plugins ship with Unluminous.

### The icons

Each plugin's icon is generated on this machine with Krea 2 through the AI service's
`POST /image-creation/generateImageToProjectFile`, which renders and writes a verified PNG straight
into the Unluminous repository. They are then keyed to transparency and scaled to 32x32, which is the size
a tab and an explorer row need.

An icon that fails to load, or a plugin with no icon, falls back to the coloured square the explorer
already draws. A missing picture must never be a missing row.

---

## 7. What is deliberately not in this

Named so that they are decisions rather than omissions.

- **Code folding.** The gap beside the line numbers is where its arrows go, and nothing else about
  the design would have to move. It needs a notion of a block, which needs a real parser.
- **A three way merge editor.** A conflicted file opens with its markers, which is a file holding
  text.
- **A language server.** Completion, go to definition and real type information all live behind one,
  and it is a large piece of work of its own — the plugin format's `kind` field is where it would be
  declared.
- **Search and replace**, still absent, still the most obvious next thing for a code editor.
- **A network marketplace**, for the reasons in section 6.
- **Regular expression literals and JSX**, listed in the plugins' own descriptions.
- **`Delete` in the explorer's context menu**, for the reason in section 4.

---

## 8. How each piece is proved

Layer by layer, the way `CLAUDE.md` sets out.

| Piece | Proved by |
|---|---|
| Syntax tokeniser | `unluminous-core` unit tests: keywords, a keyword inside a string, a keyword inside a comment, an unterminated string, a number, a function call, each of the three languages. |
| Git plumbing | `unluminous-git` tests against real repositories built in a temporary folder: status of a clean and a dirty tree, staging, committing, amending, a branch, a merge, a conflict, a rollback, a stash, a push to a second folder acting as a remote, blame over a file with two authors. |
| Plugin loading | `unluminous-app` unit tests: a manifest parses, a bad manifest is skipped rather than fatal, an extension resolves to the right plugin, a disabled plugin resolves to none, installing writes the folder and reloads from it. |
| Tabs, gutter, menus | `unluminous-app` unit tests for the state (which tab is transient, how wide the number column is, what the context menu holds for a folder against a file), and screenshot tests for the drawing. |
| Every window | New screenshot baselines: numbered gutter, gutter with blame, three tabs with one modified, the explorer's context menu open, the Git menu open, the commit panel with changes ticked, the history window, the plugins page on both tabs, and a `.ts` file coloured. Each looked at before it is accepted, on Windows, into `tests/snapshots/windows`. |
| The whole thing | `cargo run --release` on the test repository: open three files, commit, branch, merge, roll back, stash, push to the local remote, annotate, install a plugin and watch a `.rs` file gain its colours. |

---

## 9. The order the work is done in

Each step leaves the four test layers green, because a step that leaves them red makes the next step
impossible to judge.

1. The style guide, so the rest has something to be measured against.
2. The gutter with line numbers. Small, self-contained, and it forces the editing area's geometry to
   be sorted out before anything else leans on it.
3. Tabs. This is the largest change to `UnluminousApp`'s state and everything after it wants to be built
   on top of tabs rather than retrofitted into them.
4. The context menu, the prompt and the confirmation, which git then reuses.
5. `unluminous-git`, tested on its own against real repositories, with no window involved.
6. The git window: menu, commit panel, dialogs, status bar, explorer tints, gutter change bars.
7. Blame in the gutter, which needs both the gutter and `unluminous-git`.
8. The tokeniser in `unluminous-core`, tested headless.
9. The plugin registry, the three plugins and their icons.
10. The marketplace page.
11. New baselines, the whole suite, and a run against the test repository by hand.
