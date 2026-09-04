# task-1672 — a zoom that keeps the line you were reading

What was asked for:

> When I zoom in inside of a opened file, I have to scroll back to where my cursor was when I started
> zooming.
>
> I want the zoom to keep the text under my cursor in the same place, as I'm trying to zoom in on that
> line/block of code.

## 1. Why the file ran away

Unluminate's editing area is not an `egui::ScrollArea`. Each tab holds `OpenFile::scroll`, **a number of
points from the top of the document**, and the painter draws the text at `area.top() + padding -
scroll`. That is the right thing to hold while the text is a fixed size, and the wrong thing to hold
across a change of size, because the number means something different afterwards.

A file laid out at nine points is about 5,900 points tall in `app/mod.rs`; the same file at sixteen
is about 10,500. Line 1800 is 4,700 points down the first and 8,400 down the second. Scroll stays at
4,700 while the text under it moves half a file away, so zooming in on a line shows you a line you
were not looking at, a third of the way back up the source. The complaint in the ticket is exactly
this and nothing else: the zoom itself was already right, and the size it lands on was already right.

Three separate places change the size, and all three had the fault: the pinch and `Ctrl`+wheel
(`UnluminateApp::zoom_the_text`), the keyboard (`Action::ChangeFontSize`, `Action::ResetFontSize`), and the
Settings window's font panel. They all end in `set_font_size` and then `set_the_font_everywhere`,
which is what makes one fix cover all three.

## 2. What to remember instead of a scroll position

Three ways of putting the view back were weighed.

**Scale the scroll position.** `scroll * (new_height / old_height)`. One line of code and no new
state, and it is wrong in the way that is hardest to see: the ratio is right on average over the whole
document and wrong at any particular point in it. A file whose paragraphs wrap differently at the new
size — which is every file, since wrapping is what the width buys — moves lines around relative to
each other, so the further from the top the reader is, the further out the answer drifts. Rejected: a
zoom that is nearly right is a zoom you still have to scroll after.

**Remember the byte offset under the point and ask for its caret.** `Layout::caret_at` gives a
position for any offset, so `offset_at(pointer)` before and `caret_at(offset)` after is an anchor
with no new types at all. This is very nearly the answer, and it is what was built — with one
correction. `caret_at` returns the **glyph box**: `line.y + baseline - ascent`. Anchoring on that
throws away where in the line the point actually was, so a point three quarters of the way down a
line comes back at its top, and each step of a gesture loses a fraction of a line. Over the eight or
nine steps of a real pinch that adds up to a line or two of drift — small, but the ticket is about
drift.

**Remember the line and how far down it the point was.** What shipped. `unluminate_core::Anchor` is two
numbers:

```rust
pub struct Anchor {
    /// Where the line the point fell on starts.
    pub offset: usize,
    /// How far down that line the point sat, from 0 at its top edge to 1 at its bottom.
    pub fraction: f32,
}
```

`Layout::anchor_at_y` takes one and `Layout::y_of_anchor` puts it back, and both are `unluminate-core`
functions tested with no window, no fonts and no graphics card. The offset is the **start of the
line** rather than the offset under the point, so a paragraph that wraps differently at the new size
still has an answer: the line holding that byte is worked out again rather than remembered. The
fraction is what makes a step exact rather than nearly exact, because a line that is drawn 1.25 times
taller keeps the point 1.25 times further down itself.

### The off-by-one line that has to be got right

`Layout::line_of_offset` says that an offset on a line break belongs to the **earlier** line, which is
right for a caret: a caret at the end of a line is on the line it is ending. It is wrong for an
anchor. Where a paragraph wraps, the second line starts at the byte the first one ends at, so an
anchor taken on a wrapped continuation line resolves to the line above it — and the view creeps one
line up the file for every step of the gesture.

`Layout::line_of_anchor` is the same question asked the other way round: **a line that starts at the
offset wins**, and anything else falls through to the ordinary answer. That also keeps an empty
paragraph, whose line starts where it ends, a line of its own.
`an_anchor_on_a_wrapped_line_lands_on_whichever_line_now_holds_it` is the test.

## 3. Where the anchor lives, and why on the tab

An anchor is taken **before** the size changes, because it has to describe the layout the reader can
still see, and used **after** the file has been laid out again — which is the next frame at the
earliest, and for a tab in a pane nobody is looking at may be minutes later. So it cannot be a local
variable, and it is not the window's either: it belongs to the tab, beside the scroll position it
exists to correct.

`OpenFile` carries two, one for each thing that scrolls: `zoom_anchor` for the source and
`preview_anchor` for the Markdown preview, which scrolls on its own and is laid out from the same
base style. `ViewAnchor` is the `Anchor` plus `above` — how far below the top of the view the point
sat — and the arithmetic at the other end is one line:

```rust
file.scroll = (layout.y_of_anchor(anchor.at) - anchor.above).clamp(0.0, overflow);
```

`set_the_font_everywhere` takes one for **every open file** before it changes anything, so a tab that
was not on the screen comes back at the line it was left at rather than wherever its old scroll
position now points. That is the same rule the font itself already follows — one setting reaching
every tab — and it is why the anchor is stored rather than applied: the file in the third tab is not
laid out until somebody looks at it, and the anchor is still the right answer then, because a file
that has not been laid out again has not moved.

An anchor already taken is left alone, which is what makes the two kinds of anchor compose. The pinch
sets the pointer's anchor for the pane being zoomed *first*, and `set_the_font_everywhere` then fills
in the top-of-view anchor for everything else. The one nearest to what the reader is actually doing
wins, and both describe the same, still-current layout.

**It is cleared exactly where a document is replaced** — `OpenFiles::open` and the reload path — and
nowhere else. It must **not** be cleared by `forget_what_was_worked_out`, which is also how showing a
tab throws away what was laid out for it: a tab being shown is a tab whose document is the one it
always had, and an anchor dropped there is a tab that jumps the next time the font changes while it
is not the one on the screen. That was a real fault, found by
`a_zoom_leaves_a_tab_that_was_not_showing_at_the_line_it_was_left_at`.

## 4. Which point the zoom is about

| How the size changed | The point that stays still |
|---|---|
| A pinch, or `Ctrl` and the wheel, with the pointer over a pane | Where the pointer is |
| A pinch with the pointer over the explorer or the terminal | The top of the view of the pane with the keyboard |
| `Ctrl` and plus, minus or zero | The caret, clamped into the view |
| The Settings window's font panel | The top of the view, in every tab |

The caret for the keyboard, because a person pressing `Ctrl` and plus is working on the line they are
typing on, and the clamp is what makes a caret that is off the top or the bottom of the window anchor
the edge nearest it rather than scrolling the file to somewhere nobody asked to be.

egui reports **no pointer at all** on a frame whose only input is a wheel event, which is most of the
frames of a gesture — the note in `zoom_the_text` about eleven zoom frames out of thirty eight was
measured against the real window when the pinch was first written. So the pointer is asked for as
`hover_pos().or(latest_pos())`: where it is, or the last place it was seen.

## 5. A gesture belongs to the window, not to a pane

Found by running the real window rather than by a test. With the editing area split in two, one notch
of `Ctrl`+wheel took sixteen points to **thirty two**.

The size is one setting for the whole window, but `zoom_the_text` was called from `show_editor`, once
per pane, and each pane read the same `zoom_delta` and accumulated it into the same `zoom_pending`.
Two panes, two steps. This was true before this ticket; it only became visible because the anchoring
made a person watch a split zoom closely.

The gesture is claimed once a frame now, and **the pointer decides whose it is**:

- a pane the pointer is over takes it, about the text under the pointer;
- a pane with the keyboard offers to take it, and does at the end of the frame if no pane turned out
  to have the pointer.

The offer is settled after the pane loop rather than inside it, because a pane earlier in the row
must not claim a gesture aimed at one later in it, and which pane the pointer is over is not known
until they have all been drawn. `a_pinch_in_a_split_is_the_pointers_pane_and_steps_the_size_once` is
the test, and it checks both halves: one notch is one size, the pane under the pointer keeps the line
the pointer was on, and the other pane keeps the line at the top of it.

## 6. One frame late, and why the window has to be woken

The anchor is applied in `show_editor` straight after `refresh_layout` and before the scroll position
is read for anything else, so the wheel, the caret and the painter all see the position the zoom
asked for. That is the frame **after** the one the size changed on: `set_font_size` marks the layout
stale, and the frame it was called from goes on to draw the layout it already had.

An idle window draws nothing, so `set_the_font_everywhere` asks for a repaint. Without it the last
notch of a gesture would be left showing the text at its new size in its old place until something
else happened to wake the window — which is the original fault wearing a smaller hat.

## 7. What was measured

In the real window, on this repository, driven through the control channel and with real synthesised
`Ctrl`+wheel input:

| What was done | Before | After |
|---|---|---|
| Caret on line 1800 of `app/mod.rs`, two keyboard zooms 9pt → 13pt | line 1800 leaves the window | line 1800 within 4 points of where it was |
| Three keyboard zooms back out | — | line 1800 exactly where it started |
| `Ctrl`+wheel with the pointer 235 points down the pane, 9pt → 16pt | a different line under the pointer | the same line under the pointer |
| One notch of `Ctrl`+wheel over one pane of a split | 16pt → 32pt | 16pt → 20pt, both panes keeping their place |

Nine tests were added and all of Unluminate's 210 screenshot tests still pass:

- four in `unluminate-core` for the anchor itself, including the wrapped line and the empty document;
- five in `unluminate-app` driving the real window through `egui_kittest` — the pinch, the keyboard, a tab
  that was not showing, the Markdown preview, and the split.

## 8. What was deliberately not done

**The picture and the diagram views were left alone.** They zoom about their own centre, which is
`components::picture_view` and `components::diagram_view`'s business and a different gesture on a
different kind of content. The ticket is about a line of code in a file.

**Nothing is written to disk.** An anchor is a fact about a frame or two, not about a project, so it
is not in `.unluminate/`. What survives a restart is the font size, exactly as it did before.

**The horizontal position is not anchored.** Unluminate's editing area wraps rather than scrolling
sideways, so there is nothing to keep still.
