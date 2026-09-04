# task-1673 — the two halves scroll together, tabs are dragged, and every document has a scrollbar

## 1. What was asked

> When the markdown is shown in split view with the preview on the right, and I scroll, both
> documents should scroll in sync.
>
> I should also be able to drag to rearrange tabs, and drag tabs from one split pane to the other.
>
> Documents should have a thin dark scrollbar on the right side, that I can drag to scroll the
> document, and should fade/become more subtle when not hovered over and when not scrolling, but
> should become slightly more visible when hovering or scrolling.
>
> Get rid of the + new file button next to the project name, it doesn't work anyways.
>
> I should be able to right click the project name and see the popup window i see when right
> clicking other files/folders in the project.

Five asks. Two of them are small and are recorded at the end. The other three each needed something
that did not exist, and this is what was weighed for each.

## 2. Scrolling the source and its preview together

### 2.1 Why a proportion of the height is the wrong answer

The obvious way is the one a browser preview usually takes: scroll the other half to the same
fraction of its own scrollable height. It is one line of arithmetic and it needs nothing new.

It is also wrong, and wrong in a way that gets worse the further down a document you go, because the
two pages are nothing like the same shape:

| One line of source | On the source page | On the preview page |
|---|---|---|
| `## A heading` | one line | one line, half as tall again, and the hashes are gone |
| ` ``` ` | one line | nothing at all |
| `![picture](a.png)` | one line | as tall as the picture is drawn |
| a ```mermaid fence, six lines | six lines | one paragraph as tall as the diagram |
| a long paragraph | wraps at the source pane's width | wraps at the preview pane's width, in a different font |

Measured on a plain document of sixty sections — no pictures, no diagrams, nothing awkward — the
source page came out 4,819 points tall and the preview 5,460. That is a thirteen per cent difference
with nothing unusual in the file at all, so a heading a reader had scrolled to the top of the source
would be most of a screen away in the preview by the middle of the document. With a picture or two in
it the two numbers stop being comparable at all.

### 2.2 What both halves do agree about

The text. The preview is produced from the source line by line, so every line of the preview came
from some line of the source, and that correspondence is exact — it just was not written down.

So `unluminate_core::markdown::Preview` gained a fourth structure beside the text, the spans and the
paragraph styles:

```rust
/// Which line of the source each line of the preview came from, one entry a line.
pub source_lines: Vec<usize>,
```

`render` sets `builder.source_line` once at the top of its walk and `end_line` records it, so all
nine branches of the walk get it without any of them being asked to remember. Two properties follow
from the source being read from the top down and are asserted:

- **It never goes backwards**, which is what makes finding a line a binary search.
- **It is exactly as long as the preview has lines**, which the existing
  `the_three_structures_always_agree_with_each_other` test now checks alongside the other three. A
  fourth structure that could drift out of step with the other three would be a scroll to a paragraph
  that is not there.

It is not one to one in either direction, and the two awkward cases have an answer written down. A
fence's backticks produce no preview line, so a source line with no preview paragraph of its own maps
to **the last one at or above it** — which is where a reader scrolling through the backticks is
looking anyway. And a whole Mermaid fence produces a single preview paragraph, named after the line
the fence **opened** on rather than the one that closed it, because the opening is what a reader
scrolling to the diagram is aiming at.

### 2.3 The crossing

`unluminate_core::scroll_sync` is two pure functions and no state:

```rust
pub fn preview_y_for_source_y(source: &Layout, preview: &Layout, source_lines: &[usize], y: f32) -> f32
pub fn source_y_for_preview_y(source: &Layout, preview: &Layout, source_lines: &[usize], y: f32) -> f32
```

Each is three steps: which paragraph is at this height and how far down it the point sits, which line
of the other page that is, and where that line ended up at the same fraction.

**A paragraph, not a line**, and that matters. One source line is one paragraph and may wrap to five
lines at one width and to two at another, so a line number means nothing across two layouts while a
paragraph number means the same thing in both. `Layout` gained `paragraph_band` and `paragraph_at_y`
for it — the same pair, one taking what the other gives back.

**The fraction is what makes it smooth rather than stepped.** Without it the other half would jump a
paragraph at a time, which on a page of long paragraphs is a jump of most of a screen.

Both functions are pure and take laid out pages, so the whole of this is tested in `unluminate-core` with
no window: the top of one page is the top of the other, a heading scrolled to the top of the source is
at the top of the preview — with an assertion that the two pages really are different heights above
it, or the test would prove nothing — going across and back never lands further down the file, and a
document with nothing in it has an answer rather than a panic.

### 2.4 Which half drives

`UnluminateApp::scroll_the_two_halves_together` compares where both halves are **before** the frame draws
anything against where they are after, and the one that moved drives the other.

That rule is not decoration. The crossing snaps to a paragraph, so a position taken across and back is
not quite the position it started at — and a rule that wrote to both halves every frame would creep
down the file on its own for as long as the window was left open. Only one half is written to, and only
when the other actually moved. `the_two_halves_do_not_chase_each_other_when_nothing_is_touched` leaves
a settled window running for thirty frames and fails if either half has moved half a point.

Two cases fall out of the comparison for nothing. **Neither moved**: nothing happens. **Both moved**,
which is what a change of font size does through `task-1672`'s anchors: nothing happens either, and
that is right, because both have already been corrected to the line they were showing.

The follower is settled after both halves are drawn, so it lands on the next frame. egui paints
continuously while a wheel is turning or a thumb is being dragged, so that frame is sixteen
milliseconds later and cannot be seen.

**The command line needed the rule split out.** `unluminate-cli editor scroll` is applied at the top of a
frame, before anything is drawn, so the frame's own before-and-after comparison sees nothing move.
`follow_the_other_half` is the half of the rule that does the work, and both callers use it.

## 3. Dragging a tab

### 3.1 The problem is that no strip can see the others

Rearranging tabs within one strip would be easy: the strip knows where it drew each tab and where the
pointer is. Dragging a tab into **another pane** is not, because each pane draws its own strip inside
a `Ui` of its own and has never heard of the others, while the pointer wanders freely between them.

Three ways were weighed.

| | What it costs |
|---|---|
| A drag payload in egui's `DragAndDrop` memory | egui's own drag-and-drop wants each drop zone to be a widget it knows about, and Unluminate's panes are painted rectangles rather than widgets. It would also put the state somewhere the window cannot see while it is deciding. |
| Every strip told about every other strip | Each pane would have to be drawn twice, or the strips laid out in a pass of their own before any of them is drawn. Both are a rearrangement of the pane loop for one gesture. |
| **The strip reports, the window decides** | One new field on `TabsOutcome` and one new struct. The pane loop already runs left to right and already collects what each pane did. |

The third is what every other component in Unluminate already does — "components take a rectangle and
return what happened" — so it is what was built.

The strip reports two things: that a tab is being carried and where the pointer is, and **where it
drew itself and each of its tabs** (`file_tabs::Strip`). The window collects one `Strip` a pane during
the loop it already runs, and `settle_the_tab_drag` runs once afterwards, which is the earliest moment
anything knows where every strip is.

### 3.2 The rules it settles by

**A tab may be dropped anywhere in a pane**, not on its strip alone. That is what IntelliJ does and it
is what a person dragging a file into the pane beside them is aiming at; where along the strip it goes
is read from the pointer's x either way.

**A tab goes after every tab whose middle the pointer has passed** (`Strip::position_at`). That is what
makes a rearrangement follow the pointer instead of jumping when it crosses an edge.

**Dropped outside every pane, nothing happens** — over the explorer, the terminal, the status bar — so
a drag can be thought better of.

`OpenFiles::drag_tab` does the move, and it is the same call `unluminate-cli tab move` makes, so a
rearrangement made from a script and one made with the pointer are the same rearrangement. Its one
subtlety is written down where it is: `position` counts the target pane's tabs **as they are on the
screen now**, including the tab being moved when it is already in that pane, because that is what a
person dragging one is looking at — so a move within a pane to a place further along has one
subtracted from it there rather than at every call. Dropping a tab where it already was shows it and
moves nothing.

Everything else it inherits: a pane emptied by the drag is folded away by `tidy`, exactly as
`move_tab` already leaves it, and the tab is shown where it lands with the keyboard following it.

## 4. The scrollbar

### 4.1 Where it can go

The editing area asks for drags over the whole of its rectangle, the window's own resize grip takes
six points from the window's right edge, and a pane divider is grabbed over eight points centred on
the boundary. A bar has to clear all three.

Six points in from the right of the pane it belongs to clears every one of them: it is exactly what
`resize_edges::EDGE` takes and exactly what the activity bar's buttons are inset by, and it leaves two
points between the bar and a pane divider's grab area. It also lands inside `EDITOR_PADDING_X`, the
forty three points of clear space at the right of the text, so no letter is ever drawn underneath it.

### 4.2 Why it is two calls

Every other component in Unluminate is one function that interacts and draws. This one cannot be, and both
halves of the reason are about the order a frame is settled in:

- The **interaction** has to happen straight after the editing area's own, because egui hands a drag
  to the last widget that asked for the point and the editing area asks for all of it. A bar
  interacted with first is a bar that cannot be dragged.
- The **drawing** has to happen at the end, once the wheel, the caret and the sync between the two
  halves have all had their say. A thumb drawn from the scroll position the frame opened with is a
  frame behind the writing, which on a fast scroll can be seen.

So `grab` takes the drag and `paint` draws, and `Bar` is the geometry they share — built by
`Bar::new`, which gives `None` when the page fits, which is the one case where no bar should be drawn
at all.

The drag itself carries a grab offset in egui's memory, so the thumb does not jump under the pointer;
grabbing the **track** rather than the thumb takes hold of the middle of it, which is what dragging
from a click on the track means everywhere else. A click on the track alone jumps there; a click on
the thumb is the end of a drag that moved nothing and so must move nothing.

### 4.3 What "more subtle" means

The ask is that it fade when nothing is happening and be slightly more visible while hovering or
scrolling. What was rejected is fading the alpha of one colour: at the alpha that reads as "subtle"
against `EDITOR`, the idle thumb was nine values per channel away from the background and could not
honestly be called a scrollbar.

So the two ends are **palette colours** and the fade is everything between them: quiet, a five point
mark in `CONTROL`; used, an eight point mark in `TEXT_DIM` with the track behind it in `DIVIDER`. It
comes up the moment the page moves or the pointer arrives, stays up for 0.9 seconds after the page
last moved, and settles back over 0.45. **It is never taken away altogether**: a scrollbar that
disappears stops answering "how far through this am I", which is half of what it is for.

The moment it was last used lives in egui's memory under the bar's id, which is where
`components::modal` already keeps where a dialog was dragged to — a fact about how this window is
being used at this moment rather than anything the document or the settings should carry, and nothing
goes to disk for it. An idle window draws nothing, so the fade asks for a repaint while it is on its
way down, which is the same repaint `set_the_font_everywhere` asks for.

### 4.4 One change the bar forced

The wheel used to be gated on `response.hovered()` for the editing area and the preview. A widget
drawn over them takes the hover, so with a bar there a wheel turned with the pointer resting on the
bar scrolled nothing. It is gated on `contains_pointer()` now, which is "the pointer is over this
rectangle and no other **layer** is covering it" — so a bar in the same layer no longer takes the
wheel away from the page it belongs to, and a popup over the editing area still does.

## 5. The two small ones

**The plus beside the project's name is gone.** It was labelled `New file` and it called
`UnluminateApp::save`, which is why it never made a file. Making a file is on the right click menu, which
is now reachable from the name it sat beside.

**The project's name takes a right click** and reports the tree's root as a folder, so it opens
exactly the menu a folder row opens — `New -> File`, cut, copy, copy path, paste, rename, show in the
file manager, reload from disk, and the git submenu. The project folder is the one folder in the tree
with no row of its own, which is what made this the missing case. It takes no left click: there is
nothing to open or close about the root, which is always shown.

## 6. Reachable from the command line

`task-1661` asks that every feature be reachable from `unluminate-cli`, so both new capabilities are:

- `unluminate-cli tab move <position> [--tab] [--pane]` — what dragging a tab does, through the same
  `OpenFiles::drag_tab`.
- `unluminate-cli editor scroll [--line|--to|--top|--bottom] [--preview]` — reads how far through the file
  each half is, and moves either. In side by side the other half follows, through the same
  `follow_the_other_half` a wheel goes through.

The second is also how the sync was verified against the real running window rather than only in a
test: `editor scroll --line 241` on a sixty section document put the source at 4,569 points and the
preview at 5,179, and the screenshot shows `## Section 57` at the top of one and **Section 57** at the
top of the other.

## 7. What is deliberately not here

**No setting to switch the sync off.** The ask is that they scroll together; a preference for a
feature nobody has yet asked to turn off is a preference to maintain for nothing.

**No sync for a Mermaid file's diagram.** A diagram is panned and zoomed like a picture rather than
scrolled like text — `OpenFile::diagram` is a different thing from `preview_scroll` for that reason —
so there is no scroll position on that side to keep in step.

**No horizontal scrollbar.** Text in Unluminate wraps, so there is nothing to scroll sideways. A picture is
panned and has no page to be part of the way down.

**No scrollbar on the terminal.** It is not a document, and its own scrollback is a separate question
from `task-1670`'s.

**A row of panes is still a row.** Dragging a tab into another pane moves it between the panes there
are; it does not create one by dropping a tab at the edge of the window, which would be a second way
of splitting beside `Split Right` and a decision about a tree of panes that
`tasks/task-1664-split-view-tdd.md` §4 deliberately left alone.
