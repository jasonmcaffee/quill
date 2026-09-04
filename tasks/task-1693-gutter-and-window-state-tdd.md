# task-1693 — the gutter that drifts, the window that will not resize, and the state that is not kept

Nine reports in one ticket. Seven of them are small and independent; two — the resize and the session
restore — are the ones with a design behind them, and this document is mostly about those.

The list, in the ticket's own order, with the section that answers each:

| Reported | § |
|---|---|
| Zooming leaves the line numbers and the breakpoints out of line with the text | 1 |
| A right click in the explorer cannot make a **folder** | 2 |
| The window cannot be resized from the top or the bottom | 3 |
| Quitting and starting again does not bring back the windows, the files, the scroll position or the terminals | 4 |
| A file or a folder an agent makes does not appear in the explorer | 5 |
| Two rows in the explorer are highlighted at once, and one of them stays highlighted after its tab is closed | 6 |
| The run widget moves along the title bar as the open file changes | 7 |
| A right click in the empty space below the rows opens nothing | 8 |

---

## 1. The gutter is measured against the line box, and the letters do not fill it

### What was measured

`unluminate_core::layout` gives every `PlacedLine` four numbers: `y`, `height`, `baseline` and the pair
`ascent`/`descent`. They are not the same box. The **glyphs** occupy `ascent + descent` below the
line's top — the baseline is `ascent` from the top, deliberately, so that "extra line spacing is
added below the text rather than above it, so single and double spaced paragraphs start at the same
place". The **line** is taller than that, because `line_metrics` adds the font's own line gap *and*
`text_renderer::READING_LEADING`, which is `0.45 × the point size`:

```rust
line_gap: scaled.line_gap() + style.size * READING_LEADING,
```

So the empty air at the bottom of every line is about **45% of the point size**, and it grows with
the zoom.

`components::gutter` draws every one of its marks against the **line box**:

```rust
// draw_number
row.top() + (row.height() - galley.size().y) / 2.0
// draw_breakpoint
Pos2::new(..., row.center().y)
// the folding arrow
Pos2::new(change_x - ARROW / 2.0, y + line.height / 2.0)
```

Centre a mark in a box whose bottom half is empty and the mark sits low. At the default 16 points the
error is a little over three points and reads as sloppiness; at 32 it is six and reads as a fault; at
the 144 the settings allow it is thirty. That is the report exactly: *when I zoom in/out, the
breakpoints, line numbers, etc don't line up with the text*.

The number has a second problem of its own. `NUMBER_SIZE` is the constant `11.5` whatever the editor
is set to, so at 40 points the numbers are a third the height of the lines they count.

### What is done

**Every mark in the gutter is centred on the text band rather than on the line.** One function,
`gutter::text_band(line)`, returns `y + baseline - ascent .. y + baseline + descent` — the box the
letters really occupy, which is what `layout.rs` already says the caret is drawn to and for the same
reason. The number, the breakpoint dot, the execution arrow, the conditional badge, the folding arrow
and the blame cell's text all take their centre from it.

Two things deliberately keep the whole line: the **change bar**, which marks a line rather than its
letters and should meet the bar above it with no gap, and the blame cell's **background**, for the
same reason.

**The numbers and the blame column scale with the editor's font.** `Gutter` gains a `font_size`, and
the two sizes are ratios of it:

```rust
const NUMBER_RATIO: f32 = 11.5 / settings::DEFAULT_FONT_SIZE;   // 0.71875
const BLAME_RATIO:  f32 = 10.5 / settings::DEFAULT_FONT_SIZE;
```

At the default 16 that is exactly the 11.5 and 10.5 the gutter has always used, so **nothing about
the accepted screenshots changes except the alignment** — which is the point of expressing it as a
ratio of the default rather than picking two new numbers. They are clamped to a floor and a ceiling
so that six point text still has a legible gutter and 144 point text does not have a gutter wider
than the editing area. `BLAME_WIDTH` scales with `BLAME_RATIO` for the same reason: it was measured
against `12/31/2026  Firstname` at 10.5 points, and that measurement is a measurement of the type
rather than of the column.

### Why not the other two answers

**Give the numbers the editor's own font at the editor's own size, and share the baseline.** This is
what IntelliJ does and it is tempting. It costs the gutter a great deal of width — a proportional
editor font is far wider per digit than the monospace the gutter uses — and it makes the gutter's
width depend on which family is chosen, so switching the editor font would move the text sideways.
The ratio keeps the numbers monospace, which is what makes a column of them line up at all.

**Take the leading off the line instead.** Halving `READING_LEADING`, or splitting it above and
below, would make the line box and the glyph box agree. It would also change every line's position in
every document, break `task-1666`'s incremental layout fingerprints against their accepted values,
and make prose read worse — the leading is there because a font's own line height is tiring to read
at length. The gutter is what is wrong, so the gutter is what changes.

---

## 2. `New -> Folder`

`actions::explorer_menu`'s `New` submenu holds one entry, `File`. It gains `Folder`, which is
`Action::NewFolder(folder)` and `prompt_dialog::Purpose::NewFolder(folder)` beside the two the
`NewFile` pair already are — the same prompt, the same `free_name`, and `create_dir_all` in place of
`fs::write`. Making the folder opens it out in the tree and selects it; there is nothing to open in a
tab.

`Action::NewFolder(_)` is named `new-folder`, so `unluminate-cli action` reaches it the day it exists,
which is `action_names.rs`'s whole purpose. Two commands are added beside it for an agent that wants
the folder rather than the dialog — `explorer new-file <path>` and `explorer new-folder <path>` —
because "the agent creates a new file or folder" is a sentence in this very ticket and there was no
way to do it from the command line at all.

---

## 3. The window that could not be resized, and the flag that stays latched

### What was measured

Driven with real mouse input against a real window on this machine — `SetCursorPos` and
`mouse_event`, the window brought to the front first — a freshly started Unluminate resizes correctly from
**all four edges**:

```
TOP    : W 1100->1100  H 720->660     (dragged the top edge down 60)
BOTTOM : W 1100->1100  H 660->720
LEFT   : W 1100->1040  H 720->720
RIGHT  : W 1040->1100  H 720->720
```

And in `egui_kittest`, all eight grips report `drag_started` when they are dragged. So neither the
component nor the platform plumbing is broken in the ordinary case.

What *is* broken is what happens when the window manager **refuses** the request. `BeginResize`
becomes `winit`'s `handle_os_dragging`:

```rust
{
    let mut guard = window_state.lock().unwrap();
    if !guard.dragging { guard.dragging = true; } else { return; }
}
ReleaseCapture();
PostMessageW(window, WM_NCLBUTTONDOWN, wparam, points);
```

and the only place in the whole of winit that puts `dragging` back is `WM_EXITSIZEMOVE`. A posted
`WM_NCLBUTTONDOWN` that does not start a modal size/move loop never produces one — and a **maximised**
window is exactly that case: `DefWindowProc` turns the hit-test into `SC_SIZE`, and `Size` is
disabled on a maximised window. The flag latches, and from that moment **the window can no longer be
resized or moved at all**, because every later `BeginResize` *and* every `StartDrag` from the title
bar hits the early `return`.

That is what was seen live. In the run where the window was maximised and an edge was dragged, every
subsequent drag — the edges *and* the title bar — did nothing for the rest of that process's life; a
freshly started Unluminate worked again immediately. Two separate instances behaved the same way.

It also explains the shape of the report. One refused drag is enough, it is silent, and afterwards
nothing about moving or resizing works until Unluminate is restarted — so which edge a person notices
first is a matter of which one they happened to try.

### What is done

**A grip is not added when the window is maximised.** That is Unluminate's own rule for a control that can
never apply, made once more: a maximised window has no size to change, so there is nothing to grab,
nothing sets a resize cursor over it, and — the part that matters — **no request is ever sent that
the window manager will throw away.** `resize_edges::show` takes whether the viewport is maximised
and returns at once; the window reads it from `ViewportInfo::maximized`, which egui already keeps up
to date.

The title bar's own `StartDrag` is left alone, because Windows *does* handle dragging a maximised
window: it restores it and moves it, which is a modal loop and therefore an honest `WM_EXITSIZEMOVE`.

`a_maximised_window_offers_no_resize_grips` is the test, and the reason is written down in
`resize_edges`'s own module comment so that a later change cannot quietly send the request again.

### Why not the other two answers

**Subclass the window and answer `WM_NCHITTEST` ourselves.** This is the canonical way to give a
custom-chrome window on Windows real resize borders, and it would give the system cursors and Aero
Snap for nothing. It also means taking over a window procedure that winit owns, on one platform only,
with a second copy of the hit-testing logic that has to agree with the first. The measured fault is
not that the grips do not work — they do — so this buys nothing that is wrong today and costs a
platform-specific hook in the middle of somebody else's event loop.

**Send `BeginResize` anyway and try to clear the flag afterwards.** There is nothing to clear it
with: the flag is private to winit and its only reset is a message Windows will not send.

---

## 4. What Unluminate comes back as

The ticket asks for four things at once — *the windows/projects I had open should all open, and be in
the same location and state, including open files, scroll position, terminal windows* — and Unluminate
today does about half of one of them. `services::project_state` remembers which files were open,
which folders were expanded, whether the terminal was up and how many tabs it had. It does not
remember where in a file you were, where the window was, how big it was, or that there was more than
one window at all.

### 4.1 One window's own state

Three additions to `.unluminate/workspace.conf`, in the comma-list shape `files.panes` already uses so
that an Unluminate which has never heard of them reads the file unchanged:

```text
files.scrolls = 0,412.5,0
files.carets  = 0,1180,44
terminal.tab-names = build,,server
window.x = 260
window.y = 260
window.width = 1400
window.height = 900
window.maximised = false
```

`files.scrolls` and `files.carets` are one number per line of `open-files.txt`, in the same order,
and are dropped in the same pass a missing file is dropped in — which is the rule `files.panes`
already states, and is how two parallel lists are kept from coming apart. A caret offset past the end
of the file it names is clamped rather than refused, because a file can change between two runs and
"a project that opens with nothing restored is better than a project that will not open" applies to
part of a project too.

The **caret** is remembered as well as the scroll because the two answer different halves of "the
same state": the scroll is where you were looking and the caret is where you would type. Restoring
one without the other puts the caret at the top of a file you are reading half way down, and the next
key press jumps the view.

**The geometry is the project's**, written beside the rest of its state rather than in the person's
settings file, because Unluminate's windows are one per project: remembering it per person would mean the
second window opened on top of the first. It is read in `main.rs` — which already knows the folder
before the window is built — and applied through `ViewportBuilder::with_position`,
`with_inner_size` and `with_maximized`. A position that is not on any monitor now is dropped, so a
project last used on a second screen still opens.

### 4.2 Every window

An Unluminate window is a process, which `services::launcher` records as a deliberate decision and which
nothing here changes. So "the windows I had open" has to be written down somewhere both processes can
see, and that is the person's own folder beside `recent.txt`:

```text
session.txt
------------
C:\jason\dev\unluminate
C:\jason\dev\ai-service
```

- A window **adds its project** to the list when it opens, newest last, capped at
  `SESSION_LIMIT = 8`.
- A window **leaves its line behind** when it closes.
- Starting Unluminate **with no folder named** — which is the shortcut case, and is exactly the condition
  `starting_folder` already tests for by asking whether the current directory is the folder
  `unluminate.exe` lives in — opens the first line itself, starts a process for each of the others through
  `launcher::open_window`, and **rewrites the file to exactly that list**, dropping any folder that
  is no longer there.
- Starting Unluminate **with a folder named** — `unluminate .`, a file dropped on the icon, `unluminate-cli launch`
  — restores nothing and only adds itself.
- **A project that already has a window is skipped**, live windows being what
  `unluminate_cli::client::running` answers. Two Unluminates on one folder would be two processes writing one
  `.unluminate` folder and the last one to write would win, which is the same reason `OpenFiles::open`
  shows a file that is already open rather than opening it twice. `running` rather than `listed`,
  because a window that was *killed* leaves its instance file behind and a project skipped on the
  strength of a dead window is a project that never comes back.

**The trade-off is stated rather than hidden.** A line is kept when a window is closed, so closing one
window while another is open still brings both back next time. That is what the ticket asks for in as
many words, and it is the only rule available: Unluminate has no application-wide quit to hang the
question on, and by the time the last window closes the ones that closed before it are long gone from
any live registry. The cost is that a folder opened once stays in the list until eight others push it
out. The cap and the rewrite-on-restore are what bound it, and the file is one path a line in the
person's own settings folder, which is where every other list Unluminate keeps already is.

`a_window_started_on_a_named_folder_restores_nothing` and
`restoring_rewrites_the_list_to_what_was_restored` are the tests, and neither needs a window: the
list is `services::store`'s business and the spawning is `launcher::command_for`, which is already
split from running it for exactly this reason.

### 4.3 What is deliberately not restored

The terminals come back as the same number of fresh shells **with the names they were given**, which
is what `task-1682` made a name for. What a program was doing when the window closed cannot be
brought back, and `project_state` already says so. Runs and debug sessions are not restarted, for the
reason `task-1683` gives: a run is something that was *started*, and restarting somebody's dev server
because they closed a window would be a surprise.

---

## 5. The explorer notices what changed on disk

`FileTree` is read when Unluminate is told to read it — opening a project, a menu entry, a file operation
Unluminate itself did. An agent writing a file with its own tools is none of those, so the row never
appears.

**The folders that are showing are asked for their modification time, and the tree is read again when
one of them moves.** Creating, deleting or renaming an entry changes the modification time of the
folder it is in, on Windows and on macOS both, so the root plus each expanded folder is the complete
set of places a visible change can happen. `FileTree` records each folder's time as it reads it and
`FileTree::changed_on_disk` compares — one `metadata` call per open folder, which for a tree with
twenty folders open at `WATCH_INTERVAL = 750 ms` is twenty-seven calls a second and nothing
measurable. `app::HEARTBEAT` already wakes the window twice a second, so the poll happens without a
thread or a timer of its own.

### Why not `notify`

The `notify` crate is the right answer to *watching a tree*, and it is a dependency, a thread, a
channel and a lifecycle — plus a debounce, because `ReadDirectoryChangesW` on a `target` folder
during a build produces thousands of events a second and the tree walk that answers each one is the
expensive part. Unluminate does not need to watch a tree. It needs to notice that a **folder somebody can
see** has changed, and there are never more than a few dozen of those. Comparing a few dozen numbers
is smaller than the debounce alone would be.

---

## 6. Two pills

`explorer::file_row` fills the same pill for both marks:

```rust
if open || selected {
    ui.painter().rect_filled(pill, CornerRadius::same(5), color::SELECTED_ROW);
}
```

so the file that is **showing** and the row the explorer's **cursor** is on are drawn identically —
which is not what the file's own documentation says it does ("the file that is showing keeps its
filled pill, and the row the explorer's own cursor is on gains a one point ring"). Right click a
second file to reload the tree and that second file takes a pill of its own; close the first tab and
the cursor's pill stays, because the cursor is a different thing from the open file and is not
supposed to move when a tab closes.

**The pill means the file that is showing, and nothing else.** The cursor keeps the quiet `CONTROL`
fill the hover already uses, plus the accent ring while the explorer has the keyboard, which is what
the ring was always for. Two marks, two appearances, and the code agrees with the paragraph at the
top of its own file.

---

## 7. The run widget stops moving

`title_bar::run_rect` measures back from `tools_rect`, and `text_tools::width` is zero for a `.rs`
file, a picture or a `.txt` one and forty-odd points a button for a `.md` one. So switching tabs
slides the run widget along the bar — which is the fault `task-1658` moved the tools into the title
bar to stop, reappearing one control to the left.

**The run widget takes the right hand end and the text tools sit in front of it.** The bar then reads
project, tools, run, window buttons, and the play and debug buttons are in the same place whatever is
open. Two functions swap places and the callers are unchanged, because both already take the same
four arguments.

---

## 8. The menu from the empty space

Below the last row there is nothing to right click, so there is no way to make a file at the top of a
project whose first row is a folder without first hunting for a row that happens to be in the right
place.

**The empty space is the project folder's row**, which is the answer the heading already gives
(`task-1673`: "the project's name is a row like any other row in the tree, so it takes a right click
and opens the same menu a folder does"). The list's leftover height takes a right click and reports
`(at, root, true)` with one more thing said: that it was **not aimed at a row**.

`explorer_menu` takes that as a fourth argument, and the entries that are about a particular file —
`Cut`, `Copy`, `Copy Path`, `Rename...`, `Delete`, and the whole `Git` submenu — are **dimmed**
rather than absent. That is deliberately the other half of Unluminate's own rule: absent is for a control
that can never apply to this kind of thing, and dimmed is for one that could be used in a moment —
and every one of these is live the instant the pointer is over a row. It is also what the ticket asks
for in as many words.

`New`, `Paste`, the file manager entry and `Reload from Disk` stay live, because all four are about
the project folder and all four are what somebody who right clicked the empty space came for.

---

## 9. Tests

- `unluminate-core`: nothing changes. The gutter reads the numbers `PlacedLine` already carries.
- `unluminate-app` unit tests: the gutter's band arithmetic; the two ratios giving exactly today's sizes at
  the default font; the session list's cap, its rewrite and its refusal to restore when a folder was
  named; the scroll and caret lists surviving a file that has been deleted; the folder-time
  comparison; `explorer_menu`'s dimmed entries.
- Screenshots: the gutter at a large font size, the explorer's menu from the empty space, the two
  explorer marks side by side, and the title bar with the run widget at the end. Every accepted image
  in the set moves, because the line numbers move a few points up — each one is opened and looked at
  before it is accepted.
- The real window: the eight resize grips driven with real mouse input, a project quit and reopened
  with two windows, and a file made by another program appearing in the tree.

## 10. What was measured on the real window

All of this against `target/release/unluminate.exe`, driven through `unluminate-cli` and, where a pointer was
needed, through `SetCursorPos` and `mouse_event`.

- **The eight grips.** A freshly started window resized from all four edges: top `H 720 -> 660`,
  bottom `H 660 -> 720`, left `W 1100 -> 1040`, right `W 1040 -> 1100`. After one edge drag on a
  **maximised** window, every later drag — the edges *and* the title bar — did nothing for the rest
  of that process's life, and a freshly started Unluminate worked at once. Twice, in two processes.
- **Two windows, quit and brought back.** Projects A and B, each closed with `close-window`, then one
  launch from the folder `unluminate.exe` lives in: both windows came back, A at 500,200 sized 1300x850
  with `long.md` scrolled to 900 points and its terminal tab still called `build`, B at 120,90 sized
  900x600. `long.md` kept its 900 while `thing.rs`, the tab that was showing, kept its 0 — so the
  scrolls really do belong to their own tabs.
- **A project that already has a window is not opened twice.** With a window open on A, the shortcut
  launch opened **B**. Before the liveness check it opened a second A, because the first run had been
  killed rather than closed and its instance file was still there.
- **A file made outside Unluminate.** `from-an-agent.md` and `new-folder-outside/` written from a shell:
  both were in the tree within two seconds with nothing asked of the window.
- **`explorer new-folder src/services` and `explorer new-file src/services/thing.rs`** made the
  folders above them and opened the file.
- **The gutter at 30 points** in the real window, over a real desktop:
  `_agent_output/task-1693-unluminate-gutter-and-misc/live-large-font.png`.
