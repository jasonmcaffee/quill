# task-1666 — Making the window keep up

## Introduction

The ask is one sentence: on Windows, with the Unluminous project open, a few tabs open and the background
translucent, *selecting text, scrolling and dragging the window are slow and jagged*.

That is a report of a feeling, and a feeling cannot be fixed. So the first thing built for this task
was not a fix but a measurement: `crates/unluminous-app/examples/frame_cost.rs`, which opens a real file
with the real fonts of the machine it is run on, colours it exactly as the window colours it, and
prints how long each part of one frame of the editing area takes.

The first run of it said something worse than "a bit slow":

```
crates/unluminous-app/src/app/mod.rs: 168990 bytes, 3556 lines, laid out at 900 points wide
  syntax highlight:            1.14 ms  (14143 coloured spans)
  set_syntax:                561.22 ms  (20822 style spans after)
  layout, whole document:     82.24 ms
  glyphs, whole document:      7.13 ms  (116357 glyphs)
  glyphs, one screenful:       0.07 ms  (1546 glyphs)
  one advance:                166.7 ns
  one glyph lookup:            53.0 ns
```

`Document::apply(Command::PlaceCaret)` bumps the document's revision. `UnluminousApp::colour_the_file`
and `UnluminousApp::refresh_layout` are both keyed on that revision. So **every frame in which the caret
moves — which is every frame of dragging a selection — re-tokenised the file, rebuilt every style
span and laid the whole document out again**: about 650 ms of work to paint one frame, or one and a
half frames a second. Unluminous was not a bit slow. It was doing, sixty times a second, work that should
happen when a file is opened.

This document records the eight faults that were found, what was done about each, what was measured
afterwards, and the designs that were weighed and rejected.

## Goals and non-goals

**Goals**

| # | Done means |
|---|---|
| 1 | Dragging a selection through a large source file is smooth: the frame it costs is measured in tenths of a millisecond, not hundreds. |
| 2 | Scrolling costs what is on the screen, not what is in the file. |
| 3 | Typing a character costs the paragraph that changed, not the document. |
| 4 | Dragging the window is not made jagged by anything Unluminous does inside a frame. |
| 5 | Every one of those is a **test that fails before the fix**, not a number in a document. |
| 6 | The pictures do not change: every screenshot test still matches its accepted image. |
| 7 | `frame_cost` stays in the repository, so the next change to layout or painting can be measured the same way. |

**Non-goals**

- **A different text engine.** Unluminous's layout is its own, deliberately
  (`unluminous-technical-design-document.md` §3). Nothing here replaces it; everything here makes it stop
  doing work twice.
- **Threading layout.** §9 says why a background thread would have been the wrong answer to a problem
  that was almost entirely repeated work.
- **A frame budget or a profiler inside the window.** `frame_cost` is run from the command line
  against a real file, which is enough to see a regression and costs nothing at runtime.
- **Changing what anything looks like.** A performance change that alters a pixel is a performance
  change that has to be argued about on other grounds.

## 1. The eight faults

They are listed in the order they cost, worst first. Each is a separate fault with a separate fix;
none of them is the "real" one.

| # | Where | What it was | What it cost |
|---|---|---|---|
| 1 | `UnluminousApp::colour_the_file` | Keyed on `Document::revision()`, which a caret move bumps. | The whole file re-tokenised and re-coloured on every frame of a drag. |
| 2 | `StyleSpans::set` / `Document::set_syntax` | `set` is a pass over every span, and `set_syntax` called it once per token. | O(tokens × spans): **561 ms** for 14k tokens. |
| 3 | `UnluminousApp::refresh_layout` | The same `revision()` fault. | The whole document laid out again on every frame of a drag: **82 ms**. |
| 4 | `StyleSpans::runs_in` | Walked the span list from byte zero, and layout called it once a paragraph. | O(paragraphs × spans): about 37 million iterations on this file. |
| 5 | `editor_view::paint_text` | Built a mesh for every glyph in the document, every frame. | **7.13 ms** against **0.07 ms** for the screenful actually visible. |
| 6 | `unluminous_core::layout` | Three heap allocations for every grapheme cluster: the cluster's text twice, and a `CharStyle` cloned only to be compared. | Roughly 350k allocations per layout of this file. |
| 7 | `TextRenderer::advance` / `::glyph` | Built the font key by cloning the family name, so every measurement and every glyph allocated a `String`. | **167 ns** and **53 ns** a call, times 116k. |
| 8 | `Layout::line_of_offset` / `::line_at_y` | Linear scans over every line, called while painting and on every mouse move. | Small on this file, unbounded on a larger one. |

And one more that is not in the editing area at all: the explorer worked out a decoration for every
row into a `Vec` and then looked each row up in it with a linear search, which is O(rows²) every
frame.

## 2. A caret move is not a change to the text

This is the fault behind three of the eight, and the fix is one idea: **the document counts two
revisions, not one.**

`Document::revision()` still counts every change of any kind, and everything that means "did anything
at all happen" still reads it — whether a file's marked passages need writing to `.unluminous`, whether
the window needs painting again.

`Document::text_revision()` is new and counts only the changes that alter **what the text is or how
it is formatted**: an insertion, a deletion, a style applied, a paragraph's alignment or spacing, the
syntax colours, and the state undo restores. A caret move does not bump it, and neither does marking
a passage with a highlight colour — a highlight is painted over the text and changes nothing about
where the text sits.

Layout and syntax colouring are keyed on `text_revision()`. Dragging a selection across a document
therefore does no layout and no colouring at all.

**The danger with two counters is missing a bump**, and a missed bump means text on the screen that
does not match the file — the worst kind of fault, because it looks like a drawing bug and lives in
the model. So the invariant is a test rather than a promise.
`a_layout_that_changed_means_the_text_revision_moved` applies every `Command` in turn to a document
that has something of everything in it, lays it out before and after, and fails if the layout changed
while `text_revision()` did not. A command added later that forgets to bump it fails that test the day
it is written.

## 3. Applying the syntax colours in one pass

`StyleSpans::set(range, change)` splits the span list at each end of the range, walks every span
applying the change to those inside, and merges neighbours that ended up equal. That is O(spans),
which is right for what it was written for: a person selecting a word and pressing bold.

`Document::set_syntax` used it in a loop over every token in the file. Fourteen thousand calls, each
a pass over a list the previous calls had grown to twenty thousand entries, each pass cloning a
`CharStyle` — and a `CharStyle` holds a family name, so each clone is a heap allocation. 561 ms.

`StyleSpans::set_many` replaces it: **one pass that merges a sorted list of changes into the span
list**, emitting the new list as it goes. The changes have to be in increasing order and must not
overlap, which is exactly what a tokeniser produces, and one that is out of order is skipped rather
than mis-applied. `set_syntax` now sets the base colour over the whole document — one `set` — and
then hands the tokens to `set_many`.

**Rejected: making `set` itself binary-search.** It would help, but the loop would still be
O(tokens × log spans) with a clone in it, and the real answer to "this is called fourteen thousand
times" is to call it once.

## 4. Layout reads the spans once, not once a paragraph

`runs_in` starts at the beginning of the span list every time it is asked, because a span stores a
length rather than a position — which is what makes an insertion grow one span rather than shift
every span after it, and that is worth keeping (`style.rs` says so, and it is right).

So layout, which asks for the runs of paragraph 0, then paragraph 1, then paragraph 2, was walking an
ever-longer prefix of a twenty thousand entry list three and a half thousand times.

`StyleSpans::spans()` is new: an iterator over every span as an absolute byte range paired with its
style. Layout collects it once — one pass, one allocation — and then finds each paragraph's runs with
`partition_point`, which is a binary search over a sorted list. O(spans + paragraphs·log spans)
instead of O(paragraphs × spans).

## 5. Painting what is on the screen

`paint_text` walked `layout.lines` — all of them — collecting a textured rectangle for every glyph in
the document and handing the lot to egui as one mesh. egui's tessellator culls a mesh only against
its bounding box, and the bounding box of the whole document plainly overlaps the window, so the
whole thing was appended and uploaded to the graphics card every frame.

The visible lines were already being worked out elsewhere: `Layout::visible_bytes` is a pair of binary
searches, added when highlights were, and it is what `paint_highlights` uses. It has a sibling now,
`Layout::visible_lines`, which returns the line indices rather than the bytes, and `paint_text`,
`Layout::decorations` and `Layout::selection_rects` all take a line range.

`paint_text` returns the number of glyphs it placed, which is what makes the culling **testable
without a window**: `painting_a_long_document_costs_a_screenful` builds a five thousand line layout,
paints it into a clip rectangle seven hundred points tall, and fails if more than a few hundred glyphs
were placed. Before the change it placed all of them.

The decorations and the selection rectangles matter less in milliseconds and are culled for the same
reason: a rule that says "the painter touches the lines it can see" is one rule, and a rule with three
exceptions in it is not a rule.

## 6. Layout stops allocating three times a letter

Two of the three were plain waste:

- `PlacedCluster.text` was a `String`. Nearly every grapheme cluster in nearly every document is one
  to four bytes, so it is `ClusterText` now, which holds up to twenty-two bytes inline and spills to a
  `Box<str>` for the rare longer one. It is the same twenty-four bytes a `String` was, dereferences to
  `str`, and compares against `&str`, so everything that read `cluster.text` still does.
- The line-breaking pass cloned each cluster — allocating its text a second time — to set its `x`. It
  writes `x` into the cluster it already has.
- Grouping neighbouring clusters into runs compared styles by writing
  `last.style == runs[index].1.clone()`, which allocated a family name for every cluster in the
  document purely to throw it away after the comparison. It compares by reference.

The third saving is in the fonts. `TextRenderer::advance` and `TextRenderer::glyph` built their lookup
key by cloning the family name out of the style, so measuring a letter allocated a `String` and
hashing the key hashed a string. A font face is given a small integer id the first time it is asked
for, and the renderer keeps a **one-entry memo** of the last style it resolved — compared by looking
at the family name rather than by copying it. Painting and layout both walk run by run, so every
character of a run hits that memo.

**Rejected: interning the family name in `CharStyle`.** It would remove the string from the style
altogether, which is tempting, but `CharStyle` is a public value in `unluminous-core` that tests build by
hand and the settings file writes by name, and an interner is shared mutable state in a crate whose
whole point is that it has none.

## 7. Typing costs the paragraph, not the document

Everything above makes a frame in which *nothing changed* cheap. Typing a character genuinely does
change the document, and a full layout of a file this size still costs tens of milliseconds — so a
keystroke would still have been a stutter.

Layout is already paragraph by paragraph, and a paragraph's lines depend on four things: its text, the
character formatting over it, its own paragraph style, and the width. Nothing else. So a paragraph
whose four inputs have not changed does not need laying out again.

`unluminous_core::relayout` takes the previous layout and the document as it is now, and:

1. Computes a **fingerprint** for every paragraph — a hash of its text, of the styles over it, and of
   its paragraph style. Hashing the whole of this file costs about a tenth of a millisecond.
2. Finds the longest run of paragraphs at the start whose fingerprints match the previous layout's,
   and the longest run at the end. Everything between them is laid out again; everything outside is
   kept.
3. Re-stacks the kept lines: the ones before the change are untouched, and the ones after it have
   their `y`, their byte range and their paragraph number shifted by fixed amounts, because their
   contents are by definition identical.

A prefix and a suffix rather than a general diff, because that is exactly the shape of an edit: type a
letter and one paragraph changes; press Enter and one paragraph becomes two; select twenty lines and
delete them and twenty paragraphs become none. A general diff would answer more questions than a text
editor ever asks.

**The fingerprint is derived from the state rather than reported by the editor.** `Document` could
have been made to say which paragraphs it had touched, and that would be cheaper still — but it would
be a list of eleven places to keep up to date, and the twelfth, added next month, would be the one
that forgot and left a stale line on the screen. Hashing cannot go stale. It is the same argument
`UnluminousApp::follow_the_open_file` records for deriving the reveal from the state rather than firing it
from each of the places a tab can change.

`relayout` is checked against `layout` rather than trusted: `relayout_agrees_with_layout_after_every_shape_of_edit`
makes an edit of every shape — inside a paragraph, at a boundary, a paragraph added, a paragraph
removed, everything replaced — and fails unless the incremental answer is **identical** to laying the
whole thing out from scratch.

## 8. The explorer's decorations

Not in the editing area, but on the same frame. The explorer needs a git colour and a plugin icon for
each row, and both need the window, which the explorer has already borrowed — so they are worked out
first, into a `Vec`, and looked up by a closure while the rows are drawn. The closure searched the
`Vec` linearly, comparing paths, so a project with four hundred rows open did a hundred and sixty
thousand path comparisons a frame. It is a `HashMap` now.

## 9. What was rejected

**Laying out on a thread.** `unluminous-git` and the text search both run on one, and the same shape would
have worked here: hand the document to a thread, draw the previous layout until the new one arrives.
It was rejected because it answers the wrong question. Almost all of the 650 ms was work that did not
need doing at all, and moving work that does not need doing onto a thread leaves a machine warmer and
a battery flatter for no gain. It also brings a real cost: the caret, hit testing and scrolling all
read the layout, so a frame drawn against last frame's layout has to decide what to do when a click
lands on a line that has since moved. If a future Unluminous opens files where a *first* layout is slow — a
fifty megabyte log — this is the answer, and nothing here stands in its way.

**Turning the syntax colours into a `StyleSpans` of their own.** Colour is the only thing the
tokeniser sets, so it could live beside the character formatting rather than inside it, and
`set_syntax` would then not have to merge anything. It is a bigger change than it sounds — layout, the
painter, the style menus and the preview all read one list — and `set_many` made the merge cheap
enough that the second list would buy nothing.

**Filling the Windows redirection surface less often.**
`services::windows_transparency::keep_transparent` fills the window's redirection surface once a
frame, which is what lets the desktop show through (§9.2 of the technical design document). It was
re-measured for this task and is a small fraction of a millisecond, nowhere near the reported jank, so
it stays as it is — and it stays *every frame*, because the reason it cannot be done once is
unchanged.

## 10. What it costs now

### The harness, on the same file

`cargo run --release -p unluminous-app --example frame_cost -- crates/unluminous-app/src/app/mod.rs 900`, on the
machine the report came from:

| | before | after |
|---|---|---|
| `set_syntax`, 14k tokens | **561.22 ms** | **1.44 ms** |
| `layout`, whole document | 82.24 ms | 21.26 ms |
| glyphs collected, whole document | 7.13 ms | 4.04 ms |
| glyphs collected, one screenful | 0.07 ms | 0.04 ms |
| one `advance` | 166.7 ns | 34.9 ns |
| one glyph lookup | 53.0 ns | 18.7 ns |
| `line_of_offset` at the end of the file | 1.4 µs | under 0.1 µs |

And the three gestures the ticket named, each being the work the window really does for one frame of
it:

```
  dragging a selection:        0.04 ms  (24122 frames a second)
  scrolling or dragging:       0.03 ms  (39499 frames a second)
  typing a letter:             1.68 ms  (595 frames a second)
  typing, coloured again:      4.10 ms  (244 frames a second)
```

The last line is the honest one for typing: a source file is tokenised and coloured again after every
edit, because the tokeniser reads the whole file rather than the part that changed. 4.10 ms is a
quarter of a frame at sixty a second, so it is left as it is — §11 says what would be done if it ever
were not enough.

### The real window

The harness measures the pieces. The window itself was measured too, before and after, by building
the previous commit into a worktree of its own and driving both through the control channel: the
channel is answered at the **top of a frame**, so the time from writing a request to reading its
reply is one frame of the real window with real fonts and a real graphics card. Five tabs open on this
repository, the last of them `app/mod.rs`:

| One frame of… | before | after |
|---|---|---|
| moving the caret — one frame of dragging a selection | **818.0 ms** | **20.8 ms** |
| an idle frame | 33.9 ms | 21.2 ms |
| typing a letter | 819.4 ms | 32.7 ms |

The "after" column is at the floor: an idle frame answers in 20.8 ms because the window waits for the
next vertical blank, so 20 ms is one frame at sixty a second plus a loopback round trip. There is
nothing left to win there without leaving vsync, which is not something an editor should do.

The window's redirection surface fill, which is what lets the desktop show through on Windows, was
measured again over six hundred frames of a real window at 1100 by 720: **0.036 ms**. It is now one of
the larger single pieces of work in an idle frame, which says how little is left in one rather than
that it is worth changing.

## 11. How it is tested

Four layers, as everything in Unluminous is:

- **`unluminous-core`**, with no window: the two-revision invariant, `set_many` against a loop of `set`,
  `spans()` against `runs_in`, the culled queries against the unculled ones, `ClusterText` against
  `String`, and `relayout` against `layout` for every shape of edit.
- **`unluminous-app`**, with no window: `paint_text` reporting how many glyphs it placed, the font memo
  answering the same as the uncached path, and the explorer's decorations.
- **The screenshot tests**: unchanged, and that is the point. Every accepted image still matches,
  which is what says the culling and the incremental layout draw the same picture as before.
- **The real window**: `cargo run --release`, with the Unluminous project open and several tabs, dragging a
  selection through `app/mod.rs` — and the before-and-after table in §10, taken through the control
  channel against both binaries.

Five of them fail on the code as it was, which was checked one at a time by putting the old behaviour
back and running them:

| Test | Where | What it catches |
|---|---|---|
| `moving_the_caret_is_not_a_change_to_the_text` | `unluminous-core`, `document.rs` | a caret move counting as a change to the text |
| `a_layout_that_changed_means_the_text_revision_moved` | `unluminous-core`, `document.rs` | a command that changes the layout and forgets to say so |
| `an_edit_measures_the_paragraph_it_touched_and_not_the_document` | `unluminous-core`, `layout.rs` | `relayout` quietly laying the whole document out |
| `painting_a_long_document_costs_a_screenful` | `unluminous-app`, `editor_view.rs` | the painter collecting every glyph in the file |
| `dragging_a_selection_lays_nothing_out_again_and_colours_nothing_again` | `unluminous-app`, `screenshots.rs` | the whole fault, end to end, through a real pointer drag |

The third of those is worth a note. Comparing the answers cannot tell a document that was *kept* from
one that was laid out again and came out the same, and that is exactly the difference the incremental
layout is for — so the test counts what the fonts were asked. One `advance` is asked for every
grapheme cluster that is laid out, so counting them is counting the work.

## 12. What this leaves for later

Nothing here is a problem today; each is written down so that whoever meets it knows it was seen.

- **The tokeniser reads the whole file after every edit.** 4.10 ms on a 170 kilobyte file, and it
  grows with the file rather than with the edit, so it is the next thing to become the largest item.
  The answer is the same one `relayout` uses: tokenise from the start of the line the edit was on and
  stop once the tokens agree with what was there before.
- **A first layout is still a first layout.** 21 ms for a 170 kilobyte file, which is nothing when
  opening one and would be something on a file ten times the size. §9 says the thread is the answer if
  that day comes, and why it is not the answer today.
- **A `PlacedRun` still owns a `CharStyle`, and a `CharStyle` owns a family name.** That is one
  allocation per run, twenty thousand of them for a coloured file. It is a tenth of what the per
  grapheme allocations were, which is why it was left; an `Rc<CharStyle>` would take it away and would
  touch every reader of `run.style`.
