# task-1697 — Rearranging the panels

> For each panel (files, terminal, run, etc) we want to be able to drag and drop/snap to
> top/bottom/left/right of the screen. e.g. I should be able to click the top bar of the terminal
> panel, and drag it to the right, so it snaps and is a right vertical panel. Or drag it to the left,
> and it snaps to the very left, and is side by side with the file panel, or to the right of the side
> panel. There should be blue highlighted regions to indicate where I can drag to.

Four panels, four sides, one drag. This is the design; `app/dock.rs` is the implementation of it and
`components/dock.rs` is what a person sees while a panel is in the air.

## 1. What Unluminate has today, and why it cannot answer the ask as it stands

The window's layout is written out once, in `UnluminateApp::ui`, as a run of rectangle arithmetic:

```
body      = between the title bar and the status bar
rail      = the far left of the body, ACTIVITY_BAR wide
panes     = what is left
tile      = the bottom of panes, across the whole width — the terminal, the run tile or the debug tile
upper     = panes above the tile
explorer  = the left of upper, explorer_width wide
editing   = what is left
```

Every one of those is a `let` naming one pane, and each pane's size is a field of its own in
`settings::Panes` — `explorer_width`, `terminal_height`, `run_height`, `debug_height`. The shape is
not a variable; it is spelled out. A terminal on the right is not a value this arithmetic can take.

Three further things are wired to the shape rather than to the panel:

- **The dividers.** The explorer's is a one point strip down its right edge added after the editing
  area; each tile's is a one point strip along its top edge added inside the tile's own `show`.
- **The sizes a grid is opened at.** `run_grid_size` and `terminal_rows` read `panes.run_height` and
  `panes.terminal_height`, because the tile's height is the grid's height. On the right hand side of
  a window the grid's height is the *body's* height and its width is the panel's, which those two
  functions have no way to say.
- **"The bottom of the window holds one of the three tiles and never two."** That rule is written
  three times, in `show_the_run_tile`, `show_the_terminal_tile` and `show_the_debug_tile`, each of
  which puts the other two away. It is a rule about a *strip*, worn as a rule about three panels.

So the work is not "add a drag". It is to make the shape a value, lay the window out from that value,
and leave every one of the above reading the value rather than a field.

## 2. What was weighed

### 2.1 The three Rust crates

| | What it is | Why not here |
|---|---|---|
| `egui_dock` | Docking for egui: tabs that can be torn off, dragged between splits and undocked into windows. Binary splits only — a node is left/right or top/bottom. | It owns the whole layout **and the tab bars**. Unluminate draws its own tab strip, its own title bar, its own splitters and its own status bar at absolute positions taken from `design/intial-design-screenshot.png`, and `design/style-guide.md` says what a control here is made of. Adopting it would mean adopting its furniture. |
| `egui_tiles` | rerun's tiling layout engine: full horizontal, vertical and grid layouts, drag-and-drop, resizing, a `Behavior` trait to override the look. The more capable of the two. | The same objection, and a second one: a tiling engine's shape is a **tree**, and the ask is four sides. `task-1664` already refused a tree of splitters for the editing area — "a row answers the ask and every operation on it is a small function with a unit test" — and the same is true here. Every accepted screenshot in `tests/snapshots` is a picture of the layout this would replace. |
| `egui_docking` | Multi-viewport docking, bridging `egui_tiles` for the model. | Adds viewports — panels torn off into operating system windows. Unluminate's windows are one per project (`services::launcher`), and a floating panel is a second window with no project. Out of scope and deliberately so; see §11. |

The verdict is `task-1675`'s and `task-1685`'s verdict again, for the third time and for the same
reason: **the crate's output is shaped for a different consumer**. `pulldown-cmark` was refused
because its events are shaped for HTML; a language server was refused because it answers when it
pleases. A docking crate is shaped for an application that has no layout of its own yet. Unluminate has
one, it is measured against an image, and what is actually needed here is arithmetic.

### 2.2 The two drop-target mechanics

**The dock guide** — Visual Studio's diamond of four arrows that appears in the middle of the target
and is aimed at. It is unambiguous and it is a second thing on the screen that has to be learned.

**The edge band** — VS Code's and IntelliJ's: the window's own edges are the targets, a band along
each one is the hit region, and a translucent overlay shows what will happen. IntelliJ's is described
in its own words as *"a light blue shading when it gets to a place where it can be docked"*, which is
the ask, verbatim.

**The edge band is chosen.** It needs nothing drawn that is not already a region of the window, it is
what the ask describes, and it has a property the diamond has not: the band can be made to *cover the
panels already docked to that side*, which is what makes "before the explorer or after it" a question
the pointer can answer without a second control.

### 2.3 What the highlight is a picture of

Highlighting the **band** is the cheap answer and is what most implementations do. It says where the
pointer has to be, not what will happen: with two panels already on the left, one blue band says
nothing about which side of the explorer the terminal will land on.

So the highlight is **the rectangle the panel will occupy**, and it is not a second piece of
arithmetic: the layout function is run again over the layout *as it would be after the drop*, and the
moved panel's rectangle is what is painted. There is one description of where a panel goes, and the
preview is that description evaluated. A preview that could disagree with the drop is a preview
nobody can trust, and this one cannot disagree by construction.

The four bands are still drawn, faintly, because the ask says *"blue highlighted regions to indicate
where I can drag to"* — a person has to be able to see that there are four places, not only the one
they happen to be over.

## 3. The model

```rust
pub enum Panel { Explorer, Terminal, Run, Debug }
pub enum Side  { Left, Right, Top, Bottom }

pub struct Layout { sides: [Side; 4], orders: [usize; 4] }   // indexed by Panel
```

Four panels because four is what the window has. `Panel` is not a trait and not a registry: adding a
fifth is adding a variant and letting the compiler name the places that have to answer for it, which
is the same bargain `Action` and `ViewMode` already make.

**Order is screen order, always along x.** On the left, order 0 is the outermost column; on the
right, order 0 is the column nearest the editing area, because on the right "left to right" starts
at the middle. In a top or bottom strip the panels are columns too. One rule, one axis, and the drop
position is therefore one comparison everywhere.

The default is what Unluminate has now: `Explorer → Left(0)`, `Terminal → Bottom(0)`, `Run → Bottom(1)`,
`Debug → Bottom(2)`.

**Two invariants**, kept by `Layout::tidy` after every change and asserted in the tests, which are
`OpenFiles::tidy`'s two invariants restated: the orders on a side are `0..n` with no gaps, and no two
panels on one side share an order.

## 4. Sizes: two numbers per panel, not one

A panel on the left needs a **width**; the same panel at the bottom needs a **height**. One number
cannot be both — the terminal is 260 points tall at the bottom and would be a 260 point wide column
on the right, which is half a terminal.

So each panel carries a width and a height, and which one is read depends on the side it is on. The
four keys that exist keep their meaning and their values, and four more are added:

| | width | height |
|---|---|---|
| Explorer | `panes.explorer.width` — 248 | `panes.explorer.height` — 260 *(new)* |
| Terminal | `panes.terminal.width` — 420 *(new)* | `panes.terminal.height` — 260 |
| Run | `panes.run.width` — 420 *(new)* | `panes.run.height` — 260 |
| Debug | `panes.debug.width` — 520 *(new)* | `panes.debug.height` — 300 |

The debug tile is wider than the other two because it holds two panes side by side — the frames and
the values — and `debug_panel::PANE_MIN` is what says how narrow that can get.

Where the arrangement is written down is `settings::Panes`, beside the sizes, which puts it in the
person's own `settings.conf` rather than in the project's `.unluminate`. That is deliberate and it is the
line `task-1693` already drew: the window's **geometry** belongs to the project, because Unluminate's
windows are one per project and a geometry kept per person would open the second window on top of the
first; the window's **shape** is a habit, and somebody who works with the terminal on the right wants
it on the right in every project. `panes.<panel>.side` and `panes.<panel>.order` are the two keys.

## 5. Laying the window out

`dock::regions(body, layout, showing, sizes) -> Regions` is one function, pure, tested with no
window. It takes the `panes` rectangle and gives back a rectangle for each panel that is showing and
one for the editing area.

**The strips are taken first, across the whole width; then the columns, from what is left.** That is
what Unluminate does today — the terminal spans the full width of `panes`, including under the explorer —
and it is what IntelliJ does with its bottom tool window. Doing it the other way round would move
every accepted screenshot of the terminal for no gain.

```
top strip     = the full width of panes, its height the greatest of the heights of the panels in it
bottom strip  = the same, along the bottom
middle        = panes minus the two strips
left columns  = taken from the left of middle, each its own width, in order
right columns = taken from the right of middle, in order, so order 0 is nearest the editing area
editing area  = what is left
```

**Inside a strip the panels are columns as well**, each taking its own width except the last, which
fills what is left. So the explorer dropped into the bottom strip beside the terminal is 248 points
of file tree with the terminal filling the rest, which is the right answer and needs no new number.

**The clamps are the ones already written**, generalised: the editing area never goes below
`size::EDITOR_PANE_MIN` wide, and never below 120 points tall, which is the number the three tiles
are already clamped against. A side that asks for more than there is gets what is left, sharing it
between its columns in the proportion they asked for.

## 6. The drag

**The handle is the panel's own header**, which is what the ask says: *"click the top bar of the
terminal panel"*. For the three tiles that is the 32 point strip holding the word `Terminal`, the tabs
and the buttons; for the explorer it is the row holding the project's name.

The header already holds things that take a drag of their own — a terminal tab is dragged along its
strip, and `task-1682` settled that drag inside the strip. So the handle is added to the `Ui`
**first**, over the whole header, and the tabs and the buttons are added after it. egui gives a
pointer to the last widget that asked for the point, which is the rule `components::splitter` and
`components::resize_edges` are both written around; here it is used the other way up, so that the
handle gets exactly the part of the header nothing else wanted — the heading word and the empty space,
which is precisely IntelliJ's own handle.

**The component reports and decides nothing**, which is the rule every component here follows.
`dock::handle` returns whether the panel is in the air and where the pointer is; the window collects
one from each panel it draws and `settle_the_panel_drag` runs **after every panel has been drawn**,
which is the earliest moment anything knows where all of them are. That is `settle_the_tab_drag`'s
shape and it exists for the same reason: a panel picked up in one place is dropped in another.

### 6.1 Where the pointer is aimed

A band along each edge of `panes`:

- its depth is `ZONE` points, or as deep as the region already docked to that side if that is deeper,
  so the band always covers the panels already there;
- capped at 40% of the body's extent, so the middle always exists;
- **a point in two bands belongs to the one it is deeper into**, measured as a fraction of that band's
  depth. At a corner the outer edge wins, which is what a person aiming at a corner means.

Outside every band there is no target, and letting go there leaves the panel where it was — the same
"a drag can be thought better of" rule the explorer's row drag and the tab drag already state. **The
editing area is not a dock host**: a panel dropped over the document would have to become a tab, and
a terminal in a tab strip beside a `.rs` file is a different feature.

### 6.2 Where in the side it lands

**After every panel whose middle the pointer has passed.** That is `file_tabs::Strip::position_at`'s
rule word for word, and it is why order is screen order along x on every side: one comparison
answers it for all four. The panel being carried is left out of the comparison, so the index that
comes back is a plain insertion point and no caller has to subtract one.

This is the whole of the ask's second sentence — *"side by side with the file panel, or to the right
of the side panel"* — and it needs no second control, no modifier key and no extra zone.

## 7. Two panels on one side, and the rule that had to change

**Showing a panel puts away the other *tiles* docked to the same side.** The old rule was "the bottom
of the window holds one of the three and never two", and its reason was that two character grids
stacked take the editing area below the fold. The reason is about a **strip**, so the rule follows the
strip: put the terminal on the right and it no longer competes with the run tile at the bottom, and
both are showing at once — which is the point of being able to move it.

The explorer is not a grid and never competes with anything: it is a list, it is happy at 248 points
wide, and it sits beside whatever else is on its side. That is what makes *"side by side with the file
panel"* work.

## 8. What has to stop reading a field

- `run_grid_size` and `terminal_rows` read the **rectangle the panel really has**, recorded every
  frame the way `RunPanel::tile` already is. A grid on the right is as tall as the body and as wide as
  the column, and neither of those is `panes.run_height`.
- The dividers are generated from the regions rather than written out: one along the inner edge of
  each panel, resizing the panel it belongs to. A left column's divider is on its right, a right
  column's on its left, a bottom strip's on its top; the delta's sign follows from the side, in one
  place.
- The three `show_the_*_tile` functions ask the layout which of their siblings share their side.

## 9. Reaching it without a pointer

**A right click on a panel's header, or on its button in the rail, opens the panel's own menu**: `Move
to Left`, `Move to Right`, `Move to Top`, `Move to Bottom` with the current side ticked, and `Reset
Panel Layout` under a separator. `Action::Dock { panel, side }` is what each row asks for, named
`dock-terminal-right` and read back by `Action::from_name`, so `unluminate-cli action run` reaches every one
of them.

They are **not** put on the `View` menu, and that is a decision rather than an omission. A submenu is
drawn *inline* here, so four panels' four sides would be twenty rows added to a menu that already has
thirty-odd and already scrolls — which is the exact fault `task-1686` records for the Edit menu, where
three more rows pushed `Settings` off the bottom of the window. What does go on `View` is the one row
that is worth a menu on its own: **`Reset Panel Layout`**, so there is always a way back that does not
require finding the panel you lost.

The command line is the other half and is the one an agent reads:

```
unluminate-cli panel list                          every panel, its side, its order, its size, whether it shows
unluminate-cli panel dock terminal right           move a panel to a side
unluminate-cli panel dock terminal left --position 0    ... and say where in that side
unluminate-cli panel size debug --width 640        set either measurement of any panel
unluminate-cli panel reset                         put them all back
```

`panel` is a new area in `unluminate-cli/src/catalogue.rs`, so the MCP tools carry it the day it is added
and `documentation.rs` fails until `unluminate-cli/docs/commands.md` has a section for each. `explorer
width` and `terminal height` stay exactly as they are and write the same two numbers: they are the
older spelling of two of these, and removing them would break every script that has one in it.

## 10. Tests

- **`app::dock`** — the model and the arithmetic, with no window: the two invariants after every
  move, the defaults, that docking a panel where it already is changes nothing, that a side with
  three panels lays them out left to right, that the strips are taken before the columns, that the
  editing area never goes below its minimum, and that `regions` of the default layout is exactly the
  arithmetic `UnluminateApp::ui` used to do inline.
- **The zones** — that the four bands cover the four edges, that the middle is not a target, that a
  corner goes to the band it is deeper into, and that the band covers the panels already on its side.
- **The position** — that the pointer left of the explorer's middle gives 0 and right of it gives 1.
- **Screenshots** — the terminal docked right, the terminal docked left beside the explorer, the
  explorer docked right, a panel docked top, and the drop zones as they are drawn mid-drag. Each is
  driven the way a person does it: press on the header, move, release.
- **The real window** — `cargo run --release`, drag each panel to each side and look at it.

## 11. Deliberately not here

- **Floating panels.** A panel torn off into a window of its own is a second operating system window
  with no project behind it, and `services::launcher` says a Unluminate window *is* a project. It is a
  feature of its own.
- **Panels as tabs of one another.** VS Code stacks views in one region and shows one at a time; that
  is a second arrangement of the same panels and would need a strip of its own to switch between
  them. The tile family already has "one at a time on a side" without one.
- **Splitting a side top and bottom.** IntelliJ splits its left side into an upper and a lower tool
  window. That is the tree §2.1 refuses, and the ask asks for columns.
- **Dropping onto the editing area.** §6.1.
- **The rail following its panel.** IntelliJ moves a tool window's stripe button to the side the
  window is on. Unluminate has one rail on one edge; its groups say what a panel *is*, and a rail that
  reshuffled itself as panels moved would be a second thing moving for every drag.
