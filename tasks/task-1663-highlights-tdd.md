# task-1663 — Highlighting sections of text, across as many files as you like

## Introduction

`task-1663` asks for something Unluminate has not had at all: **a way to mark a passage of text and have
the mark stay there**. Select some words, right click, choose a colour, and the background behind
those words is that colour — in this file, next time the file is opened, and written down beside the
project rather than in anybody's private settings folder. The same thing from the command line, one
range at a time or a hundred ranges across a hundred files in one go.

The ask says what it is for. A person reading an unfamiliar codebase paints the four functions a bug
could be in; an agent working through a ticket paints every place it is about to change, and the
person watching can see, in the editor, what the agent thinks the shape of the problem is. It is a
shared surface between the two, which is why the command line half is not an afterthought.

Unluminate already has the two halves this needs and has never joined them. `unluminate-core` keeps
per-character formatting as spans that grow and shrink with every edit; `services::project_state`
keeps a `.unluminate` folder beside the project holding what was open. A highlight is a span that has to
survive editing and a piece of project state that has to survive closing the window. What follows is
how those two are joined, and what was rejected on the way.

## Goals and non-goals

**Goals**

| # | Done means |
|---|---|
| 1 | Right clicking a selection in the editor opens a menu with a **Highlight** row, four colour blocks, and an icon that opens a colour wheel with an opacity control. |
| 2 | Choosing a block, or applying a colour from the wheel, closes the menu and paints that colour behind the selected text at the chosen opacity. |
| 3 | Right clicking **on a highlight** offers `Clear Highlight`, which removes the one under the pointer. |
| 4 | A file may hold any number of highlights, and any number of files in a project may hold them. |
| 5 | Highlights survive editing the file around them, saving it, closing the tab and closing the window. They are gone only when cleared. |
| 6 | Looking up what is highlighted is cheap enough to do while painting every frame, on a project of any size Unluminate opens. |
| 7 | `unluminate-cli highlight` adds, lists and clears them, one at a time and in bulk across many files, and is documented in `unluminate-cli/docs/commands.md`. |
| 8 | All four test layers stay green, and the new drawing has screenshots a person has looked at. |

**Non-goals**

- **A highlight that follows the text through a change made outside Unluminate.** A highlight is anchored
  to a byte range. Editing the file *in Unluminate* moves it, because the editor knows what changed;
  `git checkout` underneath the editor does not, and the highlight stays where the bytes used to be.
  Every editor's bookmarks behave this way, and the alternative — storing the highlighted text, or a
  hash of its neighbourhood — is a second copy of the file's contents in a file people commit.
- **Highlights shared between two people.** `.unluminate/highlights.txt` is a file in the project as
  `.idea/` is, and whether it is committed is the project's business, not Unluminate's.
- **A note attached to a highlight.** The ask is a colour. A comment thread is a different feature
  with a different surface, and `services::file_marks` is shaped so it could hold one later.
- **Highlighting in the Markdown preview or in a diagram.** The preview is worked out from the
  source and has no offsets of its own; a diagram has no text ranges at all. Highlights are a
  property of a file's bytes, so they are drawn where the bytes are: the source.
- **Overlapping highlights of two colours.** The set is kept sorted and non-overlapping. Painting
  two translucent colours over each other gives a third colour nobody chose, and `Clear Highlight`
  under the pointer would have no single answer.

## Problem statement

There is nowhere to put this. Unluminate has three things that already know something about a range of
text and none of them will do.

**`Document::chars`** is the character formatting — bold, italic, a colour per run. It covers the
whole document with no gaps, which is right for formatting and wrong for highlights: a document with
two highlighted words would hold five spans, three of them saying "nothing here". It carries no
alpha either, and its comment says so on purpose: *text in Unluminate is always fully opaque*. A highlight
is a background, and the ask is explicit that it has an opacity.

**`Document::selection`** is one range and it is the caret. There can only ever be one of it.

**`services::project_state`** is per project rather than per file, and it is a list of paths — it has
no notion of anything inside a file.

And the thing that makes this more than a list: **the text moves**. Typing a line above a highlight
must carry it down; deleting the text under it must take it away. That is exactly what `StyleSpans`
does for formatting, and it is the reason the answer belongs in `unluminate-core` rather than in a service
that watches the document from outside and tries to work out what changed.

## Architectural overview

Three pieces, in the three places the existing code says they belong.

```
unluminate-core                          unluminate-app                                unluminate-cli
----------                          ---------                                ---------
highlights::Highlights              services::file_marks::FileMarks          the `highlight`
  a sorted, non-overlapping set       every file in the project that has       area of the
  of (range, Rgba), shifted by        highlights, and .unluminate/highlights.txt    catalogue:
  every insert and delete and                                                  list, add,
  carried in the undo snapshot      components::text_menu                      clear, apply
                                      the editor's right click menu:
Document::highlights()                cut, copy, paste, four blocks,
Document::highlight(range, rgba)      the wheel icon, Clear Highlight
Document::clear_highlight_at(at)
Document::clear_highlights()        components::color_wheel
                                      a hue ring, a saturation and value
                                      square, an opacity bar, Apply

                                    editor_view::paint
                                      selection, then highlights over it,
                                      then the text
```

**Who owns the truth.** A file that is **open** is owned by its `Document`; every other file is owned
by `FileMarks`. That is one rule and it decides every awkward case. The window pushes the open
document's highlights into `FileMarks` whenever that document's revision moves, pulls them out again
when a file is opened, and `FileMarks` is what is written to the disk. A command line request aimed
at a file that happens to be open goes to the document rather than round the side into the store, so
there is never a moment where the two disagree.

**Why the ranges live in `Document`.** Because `insert` and `remove_range` are the only two places in
Unluminate that know a range of bytes moved, and they already shift `chars` and `paragraphs` there. A
highlight set shifted in the same two lines cannot drift; a highlight set maintained outside the
document would have to reconstruct what changed by comparing two ropes, sixty times a second.

**Why they are in the undo snapshot.** `Snapshot` is the whole state of the document, and undo
restores a state rather than replaying an inverse — that decision is recorded in `document.rs` and
this follows it. Undoing an edit therefore puts the highlights back exactly as they were when that
edit was made. The one consequence worth stating: undoing back past the moment a highlight was made
removes it, and redoing brings it back. That is what "put it back the way it was" means, and it is
more predictable than a set of ranges left hanging over a document that has been rewound underneath
it.

**Highlighting is not an edit.** It bumps the revision, so the window repaints and the store is
written, and it does **not** set `modified` and does **not** push an undo step. This is the rule the
editor's font already follows, for the same reason: what Unluminate saves is plain text, and a highlight
is not in it.

## Components and interfaces

### 1. `unluminate_core::highlights` — the set, and the arithmetic

```rust
pub struct Rgba { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
pub struct Highlight { pub range: Range<usize>, pub color: Rgba }
pub struct Highlights { /* Vec<Highlight>, sorted by start, never overlapping */ }

impl Highlights {
    pub fn add(&mut self, range: Range<usize>, color: Rgba);
    pub fn clear(&mut self, range: Range<usize>) -> bool;  // remove whatever the range touches
    pub fn clear_all(&mut self) -> bool;
    pub fn at(&self, offset: usize) -> Option<&Highlight>;            // binary search
    pub fn overlapping(&self, range: Range<usize>) -> &[Highlight];   // binary search, then a walk
    pub fn insert(&mut self, at: usize, len: usize);   // text was typed
    pub fn remove(&mut self, range: Range<usize>);     // text was deleted
    pub fn clamp(&mut self, len: usize);               // a file that changed underneath us
}
```

The invariant — **sorted by start, non-overlapping, no empty ranges** — is what makes every one of
these cheap and every one of them have a single answer. `add` cuts away whatever it lands on before
inserting, so highlighting over a highlight replaces it, which is what a person expects of a marker
pen. `at` is a binary search, which is what makes `Clear Highlight` under the pointer free.
`overlapping` is a binary search to the first candidate and then a walk, which is what painting uses:
a file with a thousand highlights costs the window only the dozen that are on the screen.

`insert(at, len)` is deliberately **not** the rule `StyleSpans::insert` uses. Formatting typed at the
boundary of a bold word should be bold; text typed at the very edge of a highlight should *not* be
highlighted, because a highlight is a mark somebody put on a passage rather than a property the next
letter inherits. So text inserted strictly inside a highlight grows it, and text inserted at either
end is left outside it. `remove` shrinks, and a highlight the deletion swallowed whole is dropped
rather than left as a zero width mark nobody can see or click.

### 2. `Document` — a few methods, and one line in each of two others

```rust
pub fn highlights(&self) -> &Highlights;
pub fn set_highlights(&mut self, highlights: Highlights);   // what a file being opened restores
pub fn highlight(&mut self, range: Range<usize>, color: Rgba) -> bool;
pub fn clear_highlight_at(&mut self, offset: usize) -> bool;
pub fn clear_highlights(&mut self) -> bool;
```

`insert` gains `self.highlights.insert(at, text.len())` beside the line that does the same to
`chars`; `remove_range` gains `self.highlights.remove(range.clone())` beside its twin. `Snapshot`
gains a `highlights` field, and `snapshot`, `push_undo` and `restore` carry it. That is the whole of
the change to `unluminate-core` outside the new module.

### 3. `services::file_marks` — what Unluminate remembers about a file

`project_state` remembers what is true of the **project**; this remembers what is true of a **file**,
and it is named for the thing rather than for highlights because a highlight is the first of these
rather than the only one there will ever be.

```rust
pub struct FileMarks { /* HashMap<PathBuf, Highlights>, and a dirty flag */ }

impl FileMarks {
    pub fn load(root: &Path) -> FileMarks;
    pub fn save(&mut self, root: &Path);        // writes only when something changed
    pub fn highlights(&self, path: &Path) -> Option<&Highlights>;
    pub fn set(&mut self, path: &Path, highlights: Highlights);
    pub fn files(&self) -> Vec<(&PathBuf, &Highlights)>;
    pub fn total(&self) -> usize;
}
```

**The performance story, since the ask names it.** A `HashMap` keyed by path, so a file with no
highlights costs nothing at all — it is absent, and "has this file any?" is one hash. Inside a file,
the sorted set above. On the disk, **one file for the whole project** rather than one per source
file, because six hundred source files would otherwise mean six hundred files to open when a project
opens; it is read once and written only when something changed, at the same moment `project_state` is
written — once the pointer is up, so dragging never writes. Nothing walks the project and nothing is
watched.

**The format**, `.unluminate/highlights.txt`, in the plain text spirit of the two files beside it:

```
# The highlighted passages in this project. Written by Unluminate, and safe to delete.
120 240 #E8C04A59 src/main.rs
300 312 #489FF880 src/main.rs
16 64 #7FCA9866 docs/notes.md
```

Three tokens and then the rest of the line is the path, so a path with spaces in it needs no quoting;
the path is written relative to the project wherever it can be, exactly as `open-files.txt` writes
it, so a project that moves still opens with its highlights. A line that cannot be read is skipped
rather than taken as a reason to refuse the file, which is the rule the settings file already keeps.

**Only the released binary reads or writes it.** `FileMarks::load` is called from
`UnluminateApp::restore_project` and from nowhere else, exactly as the project state is, so a test neither
reads nor writes a `.unluminate` folder and a screenshot test's sample project is not changed underneath
it. A window a test builds still has a `FileMarks` — an empty one, in memory — so every menu entry
and every command works in a test.

### 4. `components::text_menu` — the editor's right click menu

Unluminate has had no menu on the editing area at all. It gets one, built from the same
`controls::menu_rows` the other two context menus use so the rows cannot drift, with two things of
its own drawn under them: a row of four colour blocks with the wheel icon at its end, and the wheel
itself once the icon has been pressed.

```rust
pub struct TextMenu { pub at: Pos2, pub offset: usize, pub wheel: Option<Rgba> }
pub struct Outcome {
    pub chosen: Option<Action>,
    pub highlight: Option<Rgba>,
    pub wheel: Option<Option<Rgba>>,
    pub close: bool,
}
```

`offset` is where in the document the pointer was, which is what decides whether `Clear Highlight` is
offered and which highlight it takes away. `wheel` being `Some` is the wheel showing, and the colour
in it is the one being chosen — held by the window rather than in egui's memory, for the reason the
gutter's menu is: **a screenshot test cannot press the right mouse button**, and a menu that can only
be opened with the right mouse button cannot be looked at.

**The wheel is inside the same popup**, not a second one over it. `CLAUDE.md` records why: egui keeps
at most one popup open at a time, so opening a second shuts the first, which is what turned three
line spacings from a dropdown into three buttons. Pressing the wheel icon makes the menu taller
rather than opening anything.

**What a right click does to the caret.** A right click inside a selection leaves the selection
alone — otherwise the menu would open with nothing to highlight, which is the whole point of it. A
right click anywhere else puts the caret there with no selection, which is what every editor does.

### 5. `components::color_wheel` — a hue ring, a square, and an opacity bar

Drawn rather than borrowed. egui has a colour picker of its own and it is a saturation square with
two strips beside it; the ask says *a colour wheel*, and the style guide says Unluminate paints its own
controls at measured positions.

- A **hue ring**: an annulus built as one `Mesh`, a pair of vertices every few degrees, each pair
  carrying the fully saturated colour at that angle. Dragging in the ring sets the hue.
- A **saturation and value square** inside the ring, subdivided into a grid of quads so the corner
  colours interpolate smoothly. Dragging in it sets both.
- An **opacity bar** under them: a checkerboard with the chosen colour drawn over it from nothing to
  full, so what sixty per cent looks like is visible rather than a number.
- The colour written out as `#RRGGBBAA`, and one button, `Apply highlight`.

Every one of those has a name, so a test can find it: `Highlight hue`, `Highlight shade`,
`Highlight opacity`, `Apply highlight`.

### 6. The actions, and the Edit menu

The four colours and the two ways of clearing are `Action`s, which is what makes them reachable from
`unluminate-cli action run` the day they exist:

```rust
Action::Highlight(HighlightColor),   // Yellow | Green | Blue | Pink
Action::ClearHighlight,              // the one under the caret
Action::ClearHighlights,             // every one in this file
```

named `highlight-yellow`, `highlight-green`, `highlight-blue`, `highlight-pink`, `clear-highlight`
and `clear-highlights`. They sit on `Edit -> Highlight`, so both menu bars get them and `action list`
lists them without anybody writing them down a second time. The colour chosen in the wheel is not an
action — it carries a value no action name could hold — and it goes through the same
`UnluminateApp::highlight_selection`, so there is still one place a highlight is made.

**The four colours are in `theme::color`.** The palette is closed and this does not open it: the four
are accents already sampled from the design — the unsaved amber, the accent blue, git's green and
blame's pink — at an alpha that leaves the writing readable. A colour chosen in the wheel is
somebody's own mark on their own text, which is the same exception the style guide already makes for
a syntax theme's token colours, and it is written down there.

### 7. `unluminate-cli highlight` — one at a time, or a hundred at once

```
unluminate-cli highlight list [path] [--all]
unluminate-cli highlight add [path] [--from-line n] [--from-column n] [--to-line n] [--to-column n]
                               [--text <needle>] [--color <name|#rrggbbaa>]
unluminate-cli highlight clear [path] [--from-line n] [--to-line n] [--all]
unluminate-cli highlight apply [--from-file <path>] [--json-text <json>]
```

`add` with no path means the tab that is showing, which is what every other `editor` command means by
saying nothing. `--text` highlights **every occurrence** of a needle in the file, which is the most
useful bulk operation on one file and needs no thread: the file is read, scanned and put back.
`apply` is the bulk one across files — a JSON array of `{path, fromLine, toLine, fromColumn,
toColumn, color}` objects, read from a file or given inline, applied in one request, so an agent that
has worked out twenty places worth marking marks them in one call rather than in twenty.

A file named by a command that is **not open** is read from the disk to turn its lines and columns
into byte offsets, and the highlights go straight into `FileMarks`. A file that **is** open goes
through its `Document`. One function, `UnluminateApp::change_highlights(path, f)`, is where that choice is
made, so no command has to think about it.

## Data flows, risks and error handling

**Painting.** `editor_view::paint` draws the selection, then the highlights over it, then the text.
That order is not arbitrary: a highlight applied to text that is still selected has to be visible at
the moment it is applied, and a translucent colour over the selection is visible while the selection
under an opaque one would not be. The rectangles come from `Layout::selection_rects`, which already
turns a byte range into one rectangle per line and is exactly what a highlight needs, so the layout
engine learns nothing at all about highlights.

**A file that changed on disk.** Byte offsets against a file somebody rewrote outside Unluminate point at
the wrong bytes. Ranges are clamped to the document's length when they are restored, so the worst
case is a highlight in the wrong place, or one that has vanished — never a panic, and never a range
that makes the layout engine reach past the end of the rope.

**A file that is open twice.** It cannot be: `open_path_in_tab` shows the tab that already holds a
file rather than opening a second one.

**Two Unluminate windows on one project.** Each writes `.unluminate/highlights.txt` when its own set changes
and the last to write wins — which is already true of `open-files.txt`. Not solving it is deliberate:
a lock file, or merging on write, is a great deal of machinery for a case a person notices at once
and fixes by highlighting it again.

**The window drawing every frame.** Painting asks `overlapping` for the visible range only, and the
sync back into `FileMarks` is an integer comparison per open tab per frame.

## Alternatives considered

| Instead of | Why not |
|---|---|
| **Extending `StyleSpans` with a background colour.** | It covers the whole document with no gaps, so every highlight would cost three spans and a document with none would still carry the machinery. It has no alpha, on purpose. And a background is not character formatting: it is not saved, not inherited by typing, and not undone. |
| **A service that watches the document and works out what moved.** | The only honest way to do that is to diff two ropes on every change. `insert` and `remove_range` already know exactly what moved, and one line in each is the whole of it. |
| **One `.unluminate/highlights/<path>.txt` per file.** | Six hundred files to open when a project opens, a directory tree mirroring the project, and a rename that has to move a metadata file. One file, read once. |
| **A sqlite database in `.unluminate`.** | A dependency, a schema, a migration story and a binary file in a folder people put in git, in exchange for indexing a list measured in tens of entries. Everything else Unluminate remembers is plain text and this should be too. |
| **Storing the highlighted text, so a highlight can be found again after an outside change.** | A second copy of parts of the file, in a file people commit. A project's secrets would end up in it. |
| **egui's own `color_picker_color32`.** | It is a square and two strips, and the ask says a wheel. It also brings egui's own sliders and layout into a window that paints everything itself at measured positions. |
| **A second popup for the wheel.** | egui closes the first when the second opens. The three line spacings in the text options panel are buttons rather than a dropdown for exactly this reason. |
| **Highlights on the undo stack.** | Then `Ctrl+Z` after marking four passages takes four presses to reach the text, and highlighting would mark the file as having unsaved changes when nothing that gets saved has changed. |

## Testing strategy

**Layer 1 — `unluminate-core`, no window.** The set: adding, cutting an overlap away, clearing, the binary
searches, and the two shifting rules — typing inside a highlight grows it, typing at either edge does
not, deleting the text under one removes it. The document: a highlight moves by exactly the number of
bytes inserted above it, survives an edit, and comes back with undo.

**Layer 2 — `unluminate-app`, no window.** `FileMarks` round trips through a temporary folder, writes its
paths relative to the project, skips a line it cannot read, and leaves a project with no `.unluminate`
folder opening normally. The colour names parse both ways. The actions round trip through their
names, which the existing `action_names` test enforces the moment the variants exist.

**Layer 3 — screenshots.** New images, each opened and looked at before it is accepted: the menu open
over a selection with the four blocks in it; the wheel open; a file with three passages highlighted
in three colours; and the same file after `Clear Highlight`. The menu is opened by setting the
window's own state, as the gutter's menu is, because the harness cannot press the right mouse button.

**Layer 4 — the real application.** `cargo run --release`, and the command line driving it: highlight
a passage from the CLI and take a screenshot of the window to see it; highlight across several files
with `highlight apply`; close the window, open it again, and check the highlights are still there.
That last one is the only thing that proves the disk half, because no test is allowed to write a
`.unluminate` folder.

## What was built, and what the real application found

Everything above was implemented. What follows is what changed while it was being built, and what
only the running window showed.

**`Layout::visible_bytes` had to be added.** Painting asks for the marks that are on the screen, and
there was no way to ask a layout what is on the screen: `line_at_y` walks the lines, which is fine
for a click and wrong for something asked once a frame with a long file scrolled a long way down. It
is two `partition_point`s over the lines, which are sorted top to bottom because that is how they were
laid out.

**Adding a mark joins it to a neighbour of the same colour.** Marking two halves of a word in one
colour left two marks that touched, which then took two `Clear Highlight`s to remove. Adding one now
merges with either neighbour when they touch **and** the colour matches — two colours that touch stay
two marks, because they are two marks.

**`Clear Highlight` is one rule, not two.** The first version cleared the mark under the caret, which
is right for a right click outside a selection and wrong for one inside it: the caret is then at one
end of the selection, which need not be on the mark the pointer is over. It clears **the marks the
selection touches, or the one under the caret when nothing is selected** — one sentence that means the
same thing on the Edit menu, on the right click menu and from the command line, and which the right
click makes true of the pointer by placing the caret before the menu opens.

**`highlight clear --all` counted twice.** A file that is open is in the store as well — the window
pushes it there every frame — so adding the two totals together reported twice as many as there were.
It counts the open files' documents and then only those store entries whose file is not open.

**The Edit menu got longer, and a snapshot changed with it.** `settings_appearance.png` differed by
six thousand pixels, all of them one hovered row in the settings list: the `Highlight` submenu moved
`Settings` down the Edit menu, so the pointer that clicked it came to rest over a different row. The
new image was looked at and accepted.

### The real run

`cargo run --release` on a three file project, driven by `unluminate-cli` from a terminal:

- `highlight add alpha.rs --from-line 6 --to-line 10 --color blue` then
  `highlight add alpha.rs --text squared --color pink` gave **five** marks, not two — the pink cut the
  blue into three. That is the rule working, and the listing shows it plainly.
- `_agent_output/task-1663-highlights/live-marked.png` is that file in the window: the colours are
  behind the words, over a syntax coloured Rust file, and the writing is readable.
- `highlight apply --from-file marks.json --replace` marked three passages across three files in one
  request, none of which was open, and wrote `.unluminate/highlights.txt`.
- Typing a line at the top of `beta.md` from the command line moved its mark from lines 5–9 to 6–10,
  and the file on the disk was rewritten with the new offsets.
- Unluminate was then **quit and started again**, and `highlight list --all` returned the same three marks
  in the same places. `live-restored.png` is the window after that restart. That is the only thing
  that proves the disk half, because no test is allowed to write a `.unluminate` folder.

### One thing found along the way, and fixed

`services::control`'s `a_request_reaches_the_window_and_the_answer_comes_back` was **flaky before this
ticket** and became flakier with it — not because anything here touches the control channel, but
because thirteen more tests in the same binary are thirteen more threads competing for the machine.
It failed once in twelve runs of the whole library suite on an unmodified checkout, and about one run
in five with this work in it, always the same way: the caller's socket read came back
`ConnectionReset`.

It was worth ten minutes to find rather than to write down and leave, because a test that fails one
run in five is a test nobody believes. Instrumenting both ends said what it was: the reply was written
and **flushed successfully**, and the caller received `ConnectionReset` with **nothing in it at all** —
which is what Windows does when a socket is torn down with something still owing on it, because a
reset throws away whatever was queued to send.

`serve` read the request through a `BufReader` built **from the stream** and wrote the answer through
a `try_clone` of it. The `BufReader` was a temporary in an `if` condition, so the reading handle was
closed the instant the request had been read — while the caller was still waiting for its answer. It
now keeps one handle for the whole conversation and takes it back out of the `BufReader` with
`into_inner` to write through, so the socket is closed once, after the answer has gone. Measured:
**thirty five consecutive clean runs** of the library suite, against about one failure in five before.

That is one line of a different subsystem and it is recorded here rather than passed on, because the
evidence for it was gathered here.
