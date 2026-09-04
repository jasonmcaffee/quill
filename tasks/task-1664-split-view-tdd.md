# task-1664 — The explorer follows the tab, and the editing area splits into panes

## Introduction

`task-1664` asks for two things that are the same thing seen from two sides: **knowing where you
are**.

The first is small and overdue. Unluminous's explorer already draws the open file as a filled pill, but
only when its row happens to be on the screen. Open a file from `Go to File`, from `Find in Files`,
from the command line, or by pressing `Ctrl+Tab` a few times, and the pill is drawn somewhere nobody
can see — inside a folder that was never opened out, or four hundred rows below the top of a project
the size of this one. The tab strip says which file is showing and the tree says nothing. The ask is
that the tree agrees with the tabs: **the file that is showing is selected in the explorer, its
folders are opened out, and the list is scrolled far enough for it to be visible.**

The second is the one with the design in it. `task-1664` asks to be able to **right click a tab and
split the editing area**, so that two files — or three, or four — are on the screen at once, each
with tabs of its own, the way IntelliJ does it. Unluminous's editing area has been one pane since it was
written, and everything about it is phrased in terms of *the* open file: one cached layout, one
cached preview, one caret, one gutter. Splitting it is therefore not a drawing change. It is a change
to what "the open file" means.

This document says what was chosen, what was rejected, and — for the two places Unluminous deliberately
does **not** copy IntelliJ — why.

## Goals and non-goals

**Goals**

| # | Done means |
|---|---|
| 1 | The file showing in the pane that has the keyboard is drawn selected in the explorer. |
| 2 | Every folder above it is opened out, so the row exists to be selected at all. |
| 3 | The list is scrolled by the least amount that brings the row into view, and not at all when it is already visible. |
| 4 | That happens however the file came to be showing: a tab click, a tab menu, `Go to File`, `Find in Files`, `Ctrl+Tab`, `unluminous-cli tab show`, closing the tab in front of it. |
| 5 | Right clicking a tab opens a menu holding `Split Right`, `Move Right`, `Move Left`, `Unsplit`, `Unsplit All` and `Close`. |
| 6 | `Split Right` puts a second pane beside the first. Each pane has its own tab strip, its own scroll position, its own view mode and its own gutter. |
| 7 | There may be any number of panes, and the dividers between them are dragged like every other divider in Unluminous. |
| 8 | Typing goes to the pane that was last clicked in, and only that pane draws a caret. |
| 9 | The split comes back when the project is opened again, panes, tabs and widths. |
| 10 | Everything above is reachable from `unluminous-cli`, and documented in `unluminous-cli/docs/commands.md`. |
| 11 | All four test layers stay green, and the new drawing has screenshots a person has looked at. |

**Non-goals**

- **Splitting downwards, and nested splits.** IntelliJ's editing area is a tree of splitters and can
  be cut into any arrangement of rows and columns. Unluminous's is a **row of panes**, left to right.
  §4 says why, and says what would have to change if the rest is wanted later.
- **The same file open in two panes at once.** IntelliJ does this and Unluminous cannot; §3 is the whole
  answer.
- **Dragging a tab from one pane to another.** The menu moves a tab between panes, which is the
  operation; dragging is a second way of asking for it and is worth having only once the first works.
- **Remembering the split per window rather than per project.** A project is a window in Unluminous, so
  the two are the same thing today.

## 1. Following the tab: three separate faults, one rule

The explorer's row for the open file is drawn selected already — `explorer::file_row` compares each
row's path against `current` and fills the pill when they match. What is missing is everything
around it:

- a row that is inside a **closed folder** is not drawn at all, so there is nothing to fill;
- a row that is **below the fold** is drawn where nobody can see it;
- and nothing notices when the answer *changes*, so even a visible row is only right by accident.

One rule answers all three. **The window remembers which path it last revealed. When the path
showing in the pane with the keyboard is not that one, it opens out the folders above it, asks the
explorer to scroll to it, and remembers it.** That is `UnluminousApp::follow_the_open_file`, called once a
frame near the top of `ui`, next to the other two questions of the same shape — has git been asked
about this file, has this file been coloured.

Deriving it from the state rather than firing it from each of the places a tab can change is the
whole point. There are eleven of those places today (`show_tab`, the tab strip, `next`/`previous`,
`open_path`, `open_path_permanently`, `open_the_match`, `close_tab`, four CLI verbs), and the
twelfth, added next month, would be the one that forgot to call it. A comparison against what was
last revealed cannot be forgotten, and it costs one `Option<PathBuf>` comparison a frame.

**Opening the folders out is `FileTree::expand`,** which already exists and already opens every
folder above the one it is given — it was written for `unluminous-cli explorer expand`. Handed a file it
walks the components, opens each folder, and does nothing to the last one because `toggle` refuses a
row that is not a directory. So the reveal needs no tree code at all.

**Scrolling is `Ui::scroll_to_rect(row, None)`,** called on the row as it is drawn, inside the
explorer's own `ScrollArea`. `None` is the important half: it means *scroll by the least amount that
brings this rectangle into view*, so a row that is already visible does not move, and the list does
not jump every time you switch tabs. `Align::Center` would put the row in the middle of the panel and
throw away the reader's place in the tree for no reason. Both `Go to File` and `Find in Files`
already scroll their lists with `scroll_to_rect(rect, None)`, so this is the same call the rest of
Unluminous makes.

**It is a one shot, and it lasts two frames.** The explorer is handed `reveal: bool`, true only just
after the file changed. A person who closes a folder that holds the open file has closed it
deliberately, and a reveal that ran every frame would open it again before the pointer was up — the
same reason `reveal_caret`, which scrolls the editing area to the caret after `Find in Files` opens a
result, is a flag rather than a condition.

Two frames rather than one, and this was found by looking at the real window rather than by reasoning.
Revealing a file usually **opens folders out in the same frame**, so the list can grow by forty rows
between one frame and the next — and egui clamps a scroll target against the content size it measured
on the *previous* frame. The first frame therefore scrolls as far as the old, shorter list allowed and
stops short of the row. Measured: opening `crates/unluminous-app/src/components/explorer.rs` in a collapsed
tree left its row just below the fold, and a second frame put it on the screen. `REVEAL_FRAMES` is
that number, with a repaint asked for so the second frame happens in an idle window.

**When the filter box has text**, the explorer draws matches rather than the tree, and the file that
is showing may not be among them. Nothing is revealed, because there is nothing to reveal; the pill
is still drawn when the file does match, exactly as it was.

**`View -> Select Opened File`** asks for the same thing by hand, which is IntelliJ's button of that
name, and is what makes the behaviour reachable from the command line as
`unluminous-cli explorer select-open-file`. It also opens the explorer if it was hidden, because a person
who asks to be shown where a file is means it.

## 2. What a pane is

A pane is **a set of tabs and which of them is showing**. Everything else that a person would call
"the state of the editor" already lives on `OpenFile` and is therefore already per tab: the scroll
position, the view mode, the blame, the diagram's pan and zoom.

Two representations were weighed.

**A `Vec<Pane>` holding indices into the files**, which is the obvious one, and is what a first
sketch used. It was thrown away. Every index into `files` shifts when a tab is opened or closed, so
every one of the seven operations on panes — split, move, unsplit, close, open, restore, focus —
has to fix up every other pane's list, and the fix-ups are exactly the sort of thing that is right
for a month and then wrong for the one case nobody wrote a test for.

**A field on the tab saying which pane it is in**, which is what was built. `OpenFile::pane` is a
number, `OpenFiles::panes` is how many there are, and `OpenFiles::focus` is which one has the
keyboard. Nothing holds an index into anything, so inserting and removing tabs needs no bookkeeping
at all: a tab keeps its pane through every shuffle of the vector, because the answer is written on
the tab.

That leaves one question — **which tab is showing in a pane** — and it is answered the same way, by
a number on the tab rather than an index in the pane. `OpenFile::shown_at` is stamped from a counter
each time a tab is shown, and the tab showing in a pane is the one in it with the highest stamp. It
is a walk of the open tabs, which is a handful of integers, and it survives every insertion,
removal and reordering without a line of maintenance. It also gives the right answer for free in the
case that would otherwise need thinking about: close the tab that is showing, and the one that comes
forward is the one you were looking at before it, which is what IntelliJ does.

`OpenFiles::active_index` — the single most called method in the window, and the meaning of "the
open file" everywhere else in Unluminous — becomes *the highest stamped tab in the focused pane*. That
one line is what makes the whole change small: **the hundred and seven places that say
`files.active()` did not have to change**, because "the file that is showing" still has exactly one
answer. The status bar, the title bar's text tools, the Git menu, the highlight commands and the
whole command line surface go on meaning what they meant.

Two invariants keep it honest, and both are asserted in the unit tests:

- **Panes are numbered `0..panes` with no gaps**, so a pane can be found by counting from the left
  and drawn in that order. Removing a pane renumbers the ones after it, which is four lines.
- **No pane is empty.** A pane that loses its last tab is removed — except the last remaining pane,
  which is left with a fresh untitled tab. That is the rule `OpenFiles::close` already keeps for the
  window as a whole, and it is kept for the same reason: there is never a pane with nothing to draw,
  so nothing anywhere needs a special case for one.

## 3. Splitting moves the tab, because Unluminous cannot open a file twice

> **Confirmed on the ticket.** Asked about this directly, Jason's answer was: *"We don't need to
> override long standing rules about documents. In split pane, just have the selected file move to
> the new pane. And I should be able to have multiple tabs in each pane."* That is what is built and
> what the rest of this section explains.

IntelliJ's `Split Right` **duplicates**: the tab appears in both splits, and you are looking at one
file in two editors, each with its own scroll position. That is genuinely useful — the top of a
function beside the bottom of it — and Unluminous cannot do it.

The reason is a rule that predates this ticket by a long way. `OpenFiles::open` says that a file
already open is *shown* rather than opened twice, "because two tabs on one file would be two
documents over one path and saving either would throw the other away". A `Document` owns its text,
its undo history, its formatting and — since `task-1663` — its highlights. Two of them over one path
is not a split view; it is two divergent copies of a file with one name, and whichever is saved
second wins.

Three ways round it were weighed:

1. **Two `OpenFile`s sharing one `Document` behind an `Rc<RefCell<_>>`.** This is what IntelliJ
   really has — one document, several editors — and it is the right answer eventually. It is also a
   change to the type every function in `unluminous-app` takes, for a feature nobody asked for in this
   ticket: the ask is to see *N tabs* at once, not one tab twice.
2. **A second, read-only view of the same document.** Cheaper, and worse: an editor you cannot type
   in, in a window where every other pane can be typed in, is a control that behaves differently for
   a reason the person cannot see.
3. **Move the tab instead of copying it.** Which is IntelliJ's own `Split and Move Right`, sitting
   two rows below `Split Right` on the same menu.

The third is what Unluminous does, and the menu says `Split Right` because that is what a person looks
for. One sentence of divergence, recorded here and in `CLAUDE.md`, in exchange for not making every
document in the program shared to get it — and it is the answer the ticket asked for.

**The exception**, which is the case a person actually hits first: splitting a pane that holds
**one** tab would take its last tab away, remove the pane it came from, and leave the window looking
exactly as it did. So when the pane holds only that tab, the tab stays where it is and the **new pane
opens empty**, with a fresh untitled tab in it, focused. That is what a person means by "put a pane
on the right": the next file they open lands in it, because opening a file always lands in the pane
with the keyboard.

The other four entries need no exception:

| Entry | What it does |
|---|---|
| `Move Right` / `Move Left` | Move the tab that is showing into the pane beside it. Dimmed when there is no pane that way. If the pane it leaves is emptied, that pane is removed. |
| `Unsplit` | Move every tab in this pane into the pane beside it — the one on the left where there is one, otherwise the one on the right — and remove this pane. IntelliJ's `Unsplit`. |
| `Unsplit All` | Every tab into pane zero. Dimmed with one pane. |
| `Close` | Close this tab. It is the first row because a menu on a tab without one would be strange. |

**Right clicking a tab shows it first.** The menu's entries all act on "the tab that is showing",
which is what makes them parameterless — `split-right` rather than `split-right --tab 3` — and so
what makes them ordinary actions that the View menu, the keyboard and `unluminous-cli action run` can all
ask for without inventing a way to name a tab. Unluminous's editing area already sets this precedent: a
right click outside the selection puts the caret there before opening the menu, so that
`Clear Highlight` means the one under the pointer.

## 4. A row of panes, not a tree of splitters

IntelliJ can cut its editing area into any arrangement, because underneath it is a tree: every node
is either a splitter with an axis and two children, or a group of tabs. Unluminous's is a **flat row**,
left to right, and the ask — "another pane on the right that allows me to view N tabs at once" — is
exactly a row.

What a tree would cost, honestly stated, because this is the decision most likely to be revisited:
the pane a tab is in stops being a number and becomes a path through the tree; laying out becomes a
recursive walk instead of a loop over widths; `Move Right` has to say what right *means* when the
pane to the right is a column of two; and the dividers become a tree of their own, each needing the
"added to the `Ui` after the panes either side of it" rule that `components/splitter.rs` records.
None of that is hard, and all of it is a different piece of work from this one. A row answers the
ask, every operation on it is a small function with a unit test, and the persisted form is a list of
numbers rather than a nested document.

**The widths are fractions that sum to one**, one per pane, kept in `OpenFiles` and dragged through
`components::splitter` like every other divider in Unluminous. Splitting divides the pane being split in
half — its half becomes two quarters, and the panes either side of it do not move, which is what a
person expects when they split the third of four panes. A pane has a smallest width of 160 points so
a drag cannot make one disappear, and closing a pane gives its share to its neighbour.

**The dividers are added to the `Ui` after every pane**, for the reason `splitter.rs` already
records: the editing area takes drags over the whole of its rectangle, and a divider added earlier
sits underneath one and never sees the pointer. This was a real fault once, found by a test that
dragged and saw nothing move, and it is why the pane loop draws all the panes first and all the
dividers second rather than drawing each divider beside its pane.

## 5. What had to move: one cache per file

The single change with real risk in it is not the panes. It is that Unluminous kept **one** laid out
document, and it now needs one per pane.

`UnluminousApp` held nine fields that were all about *the* open file: `layout`, `laid_out_revision`,
`laid_out_width`, `layout_stale`, `preview`, `preview_layout`, `preview_revision`, `preview_width`,
`preview_pictures` and `preview_diagrams`. Every one of them is a cache keyed on a document's
revision and a width. With two panes drawing two files at two widths, a single cache is not slow —
it is **wrong** in the way a cache is wrong: pane one lays out its file, pane two lays out its own
over the top, and the next frame does it again, so a large file is laid out from scratch twice a
frame for ever.

They move onto `OpenFile`, as `OpenFile::cached`. A file's layout belongs to the file, which is
where it should have been: the fields are then keyed by the thing they describe, each pane's width
is stable frame to frame, and nothing is laid out that has not changed.

Two things fall out of the move, both of them fixes rather than costs:

- **`layout_stale` all but disappears.** It existed because "the revision counts changes to one
  document, so it starts again at one for the next document that is opened, and two documents can be
  at the same revision" — a shared cache confusing two files. A cache on the file cannot confuse two
  files. What is left of it is one flag, reset when a tab's document is *replaced* in place, which is
  the one case where the same `OpenFile` holds a different document.
- **`set_the_font_everywhere` gets less careful and more correct.** It changed the base style of
  every open file but marked only the showing one's layout stale, which was right only because the
  others had no cached layout to be wrong. Now each document's own revision bump invalidates its own
  cache, so every tab is laid out again the first time it is drawn, whether it is in this pane or
  another.

`coloured_revision` — which stops the syntax colouring running twice for one revision — moves for the
same reason and by the same argument. It was one number for the window and is now one per file, so
the file in the second pane is coloured too.

Three caches deliberately **stay** on the window: `preview_images`, `mermaid_scenes` and `icons`.
None of them is keyed on a document — they are keyed on a path, on a piece of source text, and on a
plugin id — so they are shared between panes correctly and cost nothing to keep shared.

## 6. Drawing N panes without rewriting the editing area

`show_editor` and `show_preview` are four hundred lines that say `self.files.active()` in twenty
places. Rewriting them to take a pane would have meant threading an index through every one, and
through `gutter`, `refresh_layout`, `refresh_preview`, `zoom_the_text` and the pointer handlers
underneath them.

They are not rewritten. **The pane loop borrows the focus.** For each pane in turn the window sets
`files.focus` to that pane, draws the tab strip and the editing area exactly as it drew the single
one, and puts the focus back at the end. `active()` therefore answers with that pane's file for the
duration, which is precisely what the drawing code means by it.

That is a small deceit and it is worth being plain about where it stops. Two things must not follow
the borrowed focus, and both are passed in explicitly:

- **The keyboard.** `has_keyboard` is `self.focus == Focus::Editor` *and* this pane is the really
  focused one. Without the second half, every pane would take the same key presses and draw a caret,
  which is the one bug this arrangement invites. A pane that is clicked in sets the real focus to
  itself, so it is a click that moves the keyboard, exactly as it is between the editor and the
  terminal.
- **Anything the window keeps for the frame after.** `editor_area` — which the status bar and
  `unluminous-cli editor status` read — is written by the focused pane only.

Everything else genuinely is per pane and genuinely does want the borrowed focus: the gutter, the
scroll, the wheel, the pinch zoom, the right click menu, the selection painting.

**Each tab strip is given an id of its own**, `("file-tab", pane, index)`, because egui identifies a
widget by its id and two strips whose second tab shared an id would hand the same click to both.
The strip also learns one new thing, `focused`, and draws the accent line under the showing tab in
the quiet colour when its pane does not have the keyboard — which is how a person sees at a glance
which of four panes their typing is going to.

## 7. Remembering the split

`.unluminous/workspace.conf` gains two keys and `open-files.txt` gains nothing, which matters: the file
of paths stays a file of paths, and an Unluminous that has never heard of panes reads it unchanged.

```
files.panes = 0,0,1,1
files.pane-widths = 0.5,0.5
files.pane = 1
```

`files.panes` is one number per line of `open-files.txt`, in the same order. A file that has since
been deleted is dropped from the list when the project is read, and its pane number is dropped with
it in the same pass — the two lists are filtered together rather than one after the other, which is
the kind of thing that is right until somebody adds a second filter.

Everything is defended on the way in, because a hand edited file is a file somebody may have got
wrong: a pane number past the end is clamped, a missing key means one pane, a widths list of the
wrong length is replaced by equal widths, and a set of numbers that would leave a pane empty is
collapsed. The rule the whole state file already keeps applies here too — a project that opens with
nothing restored is better than a project that will not open.

Widths are remembered as fractions rather than points so that opening the project on a screen of
another size gives the same proportions rather than the same measurements.

## 8. The command line

`task-1661` asks that every feature be reachable from the command line and be documented, and both
are tests rather than promises. Five new actions and one new area:

Actions, reachable through `unluminous-cli action run <name>` and listed by `action list` because they
are on the View menu: `split-right`, `move-tab-right`, `move-tab-left`, `unsplit`, `unsplit-all`,
`next-pane`, `previous-pane`, `select-open-file`.

The `pane` area is what a script actually wants, because it can name a pane rather than walking to
it:

| Command | What it does |
|---|---|
| `pane list` | The panes, the tabs in each, which tab is showing in each and which pane has the keyboard. |
| `pane split` | Split the pane that has the keyboard, as the tab menu's `Split Right` does. |
| `pane move <direction>` | Move the tab that is showing one pane `left` or `right`. |
| `pane focus <pane>` | Put the keyboard in a pane, by its number counting from zero. |
| `pane width <pane> <fraction>` | Set one pane's share of the editing area, which is what dragging a divider does. |
| `pane unsplit` | Fold the focused pane into its neighbour. |
| `pane unsplit-all` | Back to one pane. |

`explorer select-open-file` is the ninth, and it is in the `explorer` area rather than the `tab` one
because it is the explorer that moves. `tab list` and `status` grow a `pane` on each tab, so a
script can see the arrangement it has just made.

Every one of them goes through the same functions the menu entries do — `UnluminousApp::run_action` for
the actions and one `OpenFiles` method for each pane operation — so a split made from the command
line and a split made by right clicking a tab are the same split. That is the rule `run_cli` already
keeps.

## 9. How it is tested

The four layers, as `CLAUDE.md` requires.

**`OpenFiles`, with no window.** Twelve unit tests, and they are the ones that matter, because the
pane model is arithmetic on a vector and every awkward case is reachable without drawing anything:
splitting a pane with one tab and with three, moving the last tab out of a pane, unsplitting into
the left neighbour and into the right one, closing the showing tab in a pane with others and in a
pane without, opening a file while a second pane has the keyboard, and the two invariants — panes
numbered without gaps, no pane empty — asserted after each.

**`project_state`, with no window.** A round trip of a two pane project, a `files.panes` list that is
shorter than the file list, a pane number past the end, and the existing tests unchanged, which is
what says the format is still backwards compatible.

**The whole window, through `egui_kittest`.** Four screenshots: a two pane split with a different
file in each, a three pane split, the explorer scrolled to a file deep in a closed folder, and the
tab menu open. Each is looked at before it is accepted, because the point of the image is that a
person can see that the second pane really is drawing the second file and that the caret is in one
pane only.

**The real window.** `cargo run --release`, a split made by hand, typing in both panes, dragging the
divider, and the project closed and opened again to see the split come back.

## 10. What was built, and how it was proved

Every goal in the table above is done, and the evidence is a test rather than a claim.

| Layer | What it covers |
|---|---|
| `OpenFiles`, no window | **20 unit tests.** Splitting a pane with one tab and with three, moving the last tab out of a pane, unsplitting into the left neighbour and into the right one, closing the showing tab, each pane walking only its own tabs, a divider that cannot be dragged past its neighbour, and a state file asking for a pane that would be empty. Every one of them ends by asserting both invariants. |
| `project_state`, no window | **4 more.** A two pane project round tripping, a deleted file taking its pane number with it rather than sliding everyone else along, a pane number past the end being clamped, and a state file written before there were panes opening in one pane. |
| The whole window, `egui_kittest` | **11 tests, 4 of them screenshots** — two panes, three panes, a tab's own menu, and the explorer opened out and scrolled to a file two folders down. Also: only the pane with the keyboard takes what is typed, each pane lays its file out at *its own* width, the command line splits and refuses a pane that is not there, and a split project opens split again. |
| The command line | `unluminous-cli/docs/commands.md` is regenerated from the catalogue, and `documentation.rs` fails while any of the eight new commands has no section. |

The whole workspace is green: **1,099 tests**, no failures.

Three things the pictures caught that no assertion would have. The first is the feature working at all
in a test that was written for something else: `mermaid_side_by_side` used to show `state.mmd` open in
the tab with the explorer sitting at the top of a twenty three file list and nothing selected, and now
shows the list scrolled to it with the row filled. The second is a fault — the first two pane
screenshot came back with egui's `First use of widget ID 4F83` painted in red across it, because the
two panes' gutters were one widget as far as egui was concerned. That is what the id salt in §6 is,
and a person looking at the image is how it was found. The third is the one frame scroll of §1, which
every test passed and the real window did not.

**The real window, by hand.** `cargo run --release` on Unluminous's own repository, driven through
`unluminous-cli`: two panes, then three at unequal widths, typing in each, `unsplit-all`, and the window
closed and opened again — which brought the three panes and their tabs back off `.unluminous`. The captures
are in `_agent_output/task-1664-split-view/`.

## 11. What this leaves for later

- **Splitting downwards**, and the tree of splitters that goes with it (§4).
- **Dragging a tab between panes**, which the menu makes unnecessary but not unwanted.
- **One document in two panes**, which needs the shared `Document` of §3 option 1, and which would
  then make `Split Right` mean what IntelliJ means by it.
- **A keyboard shortcut for the splits.** None is given here on purpose: on macOS a shortcut on a
  menu item is a key equivalent, two menu items claiming one equivalent is a fault, and the obvious
  candidates are taken. It is a decision to make with the whole keyboard in view rather than one
  ticket at a time.
