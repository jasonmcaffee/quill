# Working on Quill

Read this before changing anything. It records the conventions the code already follows, so that a later
change looks like the rest of the code rather than like a second style laid over it.

## What the crates are for

| Crate | What is in it | What must never be in it |
|---|---|---|
| `quill-core` | The editor: the text buffer, the character and paragraph formatting, the caret, layout, undo, the Markdown parser, the syntax tokeniser, and the Mermaid reader and diagram layout. | Any user interface dependency. Its tests run with no window, no graphics card and no fonts. |
| `quill-terminal` | The terminal: the session over a pseudoterminal, the screen the painter reads, the colour palette, the key encoding and the mouse reports. | Any user interface dependency, for the same reason. |
| `quill-git` | Reading and changing a git repository: the status, blame, the log, diffs, branches, and every operation on the Git menu, plus the thread they run on. | Any user interface dependency, and any decision about what a dialog looks like. Its tests build real repositories in a temporary folder and ask git what happened. |
| `quill-app` | The window: drawing, input, real fonts, the settings on disk, the menus, and the plugin registry. | Editor behaviour, terminal emulation or git plumbing. Those belong in the crates above. |
| `quill-cli` | The command line: the catalogue of commands, the wire format, and the client program. It lives in `quill-cli/` beside its own documentation rather than under `crates/`, because the two are read together. | Anything that depends on `quill-app`. The dependency points one way, so the client stays a small program with no window, no graphics card and no fonts behind it. |

`quill-app` is laid out in four folders and a new file belongs in one of them:

- `app/` — the window's own state, and `app/actions.rs`, which is what the menus and the keyboard ask for.
- `components/` — drawing. One file for each piece of the window.
- `services/` — everything that is not drawing: the file tree, the fonts and the glyph atlas, the settings
  and recent projects on disk, what one project remembers about itself, decoding a picture, starting a
  second window, the macOS menu bar, the socket the command line drives the window down, and what
  Windows needs before the desktop will show through the window.
- `theme/` — the palette, the measurements and the drawn icons.

## The look is written down, and a new control is measured against it

`design/style-guide.md` says what a control in Quill is built from: the palette is closed, a list row
is 28 points and a menu row is 24, selection is one pill drawn one way, a modal has one shape, icons
are drawn rather than lettered, and every control has a plain name. **Read it before drawing anything
new**, and add to it rather than inventing a second answer.

Two things it separates that are easy to confuse. `design/` holds **intent** — the image a component
is compared against, changed only when the design changes. `crates/quill-app/tests/snapshots` holds
**accepted output** — a change that alters the rendering fails against it, and `UPDATE_SNAPSHOTS=1`
accepts a new one after somebody has opened the image and looked at it.

`components::modal` is the furniture every modal is made of: the frame, the header, the body
rectangle, the footer, the buttons, the rows, a field and a tick box. The Settings window, the commit
panel, the git dialogs, the text prompt, the confirmation, `Go to File` and `Find in Files` are all
built from it. A tenth modal that draws its own header would be a tenth modal that almost agrees with
the other nine.

**It also owns dragging and resizing**, which `task-1659` asks for: the header is the handle, eight
invisible grips round the edge resize it, and a double click on the header puts it back in the middle
at the size it asked for. They are in `modal::show` rather than in any one dialog, so a dialog written
later has them without asking — which is why `prompt_dialog` and `settings_dialog`, which each held a
copy of the frame, now call it instead. Where a modal has been dragged to lives in egui's memory under
the modal's id: the window has no decision to make about it and nothing is written to disk.

`components::activity_bar` is the thin rail down the far left, one button a pane: the explorer and git
at the top, the terminal at the bottom left. It is the only way a pane is put away and brought back,
and its buttons are inset by exactly what `components::resize_edges` takes from the window's left edge,
so a button and the window's own resize grip never want the same point.

`components::resize_edges` is the eight grips the window is resized by. Quill's window has no
operating system frame — the rounded corners and the transparency need the decorations off — so it has
no resize grip of its own and draws its own, invisibly, four edges and four corners. **They are added
to the `Ui` last**, after every pane, for the reason a divider is added after the panes either side of
it: egui gives a pointer to the last widget that wants it, and the editing area, the explorer and the
status bar all take drags over the whole of their rectangles.

`components::controls` is the same idea for the small things: the dropdown, the flyout, a menu row, a
button with a word on it, an icon button, and `field_text_rect` — the rectangle a field hands to the
`egui::TextEdit` inside it. That last one exists because all five fields in Quill had the same fault:
egui lays a text box out at the top of the rectangle it is given, `Frame::NONE` leaves no margin to
push it down, and a field that handed over its whole height put its words against its top edge. One
function is what stops a sixth field being the sixth chance to get it wrong.

**A flyout must not hold a dropdown or another flyout.** egui keeps at most one popup open at a time,
so opening the second shuts the first — which is why the three line spacings in the text options
panel are three buttons rather than the dropdown they used to be.

## Two ways of searching a project, and both of them are the modal's

`task-1659` asks for IntelliJ's two: `Go to File` on `Ctrl/Cmd+Shift+O`, which narrows the project's
files as a name is typed, and `Find in Files` on `Ctrl/Cmd+Shift+F`, which searches every file's text
and shows the whole of the chosen one underneath the results. `Open Folder` moved to
`Ctrl/Cmd+Alt+O` to make room, because two menu items claiming one key equivalent is a fault on macOS
and there is a test for it.

Neither component decides what matches. `services::file_search` ranks file names — a **subsequence**,
so `mdrs` finds `markdown.rs`, with a match in the name outranking one in the folders above it — and
`services::text_search` reads the project's text. Both are pure enough to be tested with no window,
which is where nearly all of their tests are.

**The text search runs on a thread**, arranged as `quill_git::Worker` and the terminal already are,
with a waker that asks the window to draw again. Reading a project on every key press where the
window draws would stop it drawing, which on a large folder looks exactly like a crash. Only the
newest question is answered: each request carries a number, the newest is shared with the thread as
an `AtomicU64`, and a search whose number has been passed stops where it is. That is what makes typing
quick without a debounce timer, which would be wrong at both ends — too long on a small project and
too short on a large one.

**What a build wrote is not searched.** `FileTree::all_files` leaves out `target`, `node_modules` and
`__pycache__`, which is what the filter box, `Go to File` and `Find in Files` all search. They are
still **shown** in the explorer, because it is a picture of the folder. Three names only, and each is
a folder nobody writes a source file into: `build`, `dist` and `out` are deliberately not among them,
because a search that silently missed a real file would be worse than one offering a few too many.
Measured on Quill's own repository, this took the list from 2022 files to 618 and the whole search
from 60 ms to 20 — and it fixed a real fault, because the walk gives up after a fixed number of files
and `target` had been eating that budget before the source was reached.

## Mermaid is drawn in Rust, and the painter knows nothing about diagrams

A `.mmd` file opens as a **drawn diagram** with the same three view modes a `.md` file has, and a
```mermaid fence inside a Markdown document is drawn in its preview. Twenty of Mermaid's thirty
diagram types are drawn; the other ten are **named** rather than mis-drawn, and that distinction has a
test of its own.

**None of it runs `mermaid.js`.** `tasks/quill-mermaid-plugin-tdd.md` §2 weighs the three ways of
doing that — `mermaid-cli` needs Node and a headless Chromium, embedding a JavaScript engine means
implementing enough DOM and SVG to answer `getBBox()` truthfully, and a web view puts a second
compositor inside a window whose transparency took three separate fixes to get right. What a diagram
needs on top of what Quill already has is arithmetic, so `quill_core::mermaid` does the arithmetic.
The cost is stated rather than hidden: the pictures are **not** pixel identical to `mermaid.js`, and
the bar they are held to is correct and readable.

`quill-core` reads and lays out; `quill-app` draws. The seam is a **`Scene`: five kinds of item**,
rectangles, circles, polygons, lines and text at absolute positions, and nothing else. An arrowhead is
a filled polygon of three points, a pie slice is a flattened arc, a crow's foot is three lines — all
built in `quill-core` where they can be tested with no window. So `components::diagram_view` has no
diagram knowledge at all, and a twenty-first diagram type needs no change there.

`layered.rs` is Sugiyama's layered layout, shared by the five graph-shaped types — flowchart, class,
state, ER and requirement. Two rules in it matter more than they usually would. **Every sweep count is
a constant and nothing is random**, so the same source always gives the same picture, which is what
makes a screenshot test of a diagram possible at all. And **a subgraph is laid out on its own and
placed as one box**, recursively, so its contents cannot overlap anything outside it; edges that cross
the frame are re-attached to the real nodes once everything has an absolute position, so an arrow into
a subgraph points at the box it names rather than at the frame.

**A diagram's own `style`, `classDef` and `click` are read and ignored**, which is the same decision
`services::plugins` already made about a colour scheme: a document does not get to choose the
window's colours, and nothing in a diagram is going to run. **Nothing is fetched**, ever.

The four properties every diagram type is held to are one function, `mermaid::check::properties`, so
a type added later inherits them: nothing outside the scene, no two node boxes overlapping, every
number finite — one NaN poisons the size and blanks the whole diagram — and every source label
present. The fifth, that laying the same source out twice gives an identical scene, is what the
images rest on. `sample-diagrams/` holds one file per type, and it is what both the screenshot tests
and a person read; `cargo run --example mermaid_check` lays every one of them out and says what came
of it, which is the quickest way to see that a layout change has broken nothing.

## A frame costs what is on the screen, not what is in the file

`task-1666` reported that selecting text, scrolling and dragging the window were jagged on Windows
with a few tabs open. Measured through the control channel against the real window, one frame of
dragging a selection through `app/mod.rs` cost **818 ms**. It costs 20.8 ms now, which is one frame
at sixty a second plus the loopback round trip — the floor.

Four rules came out of it, and a change to the editing area has to keep all four.

**A caret move is not a change to the text.** `Document` counts two revisions.
`Document::revision()` counts every change of any kind and is what "does the window need painting
again" reads. `Document::text_revision()` counts only what alters the text or its formatting, and is
what `refresh_layout`, `refresh_preview` and `colour_the_file` are keyed on. Keyed on the first,
every frame of a drag re-tokenised the file, rebuilt every style span and laid the whole document out
again. `a_layout_that_changed_means_the_text_revision_moved` applies every `Command` in turn and
fails if the layout came out different while `text_revision` stood still, so a command added later
that forgets is caught the day it is written.

**The painter touches the lines it can see.** `Layout::visible_lines` is a pair of binary searches
over the clip rectangle, and `paint_text`, `Layout::selection_rects_in` and `Layout::decorations_in`
all take a line range. egui culls a mesh only against its bounding box, and the bounding box of a
whole document plainly overlaps the window, so collecting every glyph in the file meant tessellating
and uploading every glyph in the file, sixty times a second. `paint_text` returns how many glyphs it
placed, which is what makes that testable with no window at all.

**An edit costs the paragraph it changed.** `quill_core::relayout` takes the previous layout and
keeps every paragraph whose text, formatting and paragraph style fingerprint the same, laying out
only the run between the longest matching prefix and the longest matching suffix. **The fingerprint
is derived from the state rather than reported by the editor**, for the reason
`follow_the_open_file` records: a list of the places that have to say "I changed this" is a list
whose next entry will be the one that forgot, and a stale line on the screen is a fault that looks
like a drawing bug and lives in the model. It is checked against a full layout for every shape of
edit and has to be **identical**, not close.

**Nothing that runs once a letter may allocate.** `StyleSpans::set_many` applies a tokeniser's whole
output in one pass rather than one pass per token — 561 ms to 1.4 ms on a coloured 170 kilobyte file.
`StyleSpans::spans()` gives layout the span list once with absolute positions to binary search,
instead of `runs_in` walking from byte zero once a paragraph. `PlacedCluster::text` is a
`ClusterText`, twenty-two bytes inline, rather than a `String`. And `TextRenderer` gives each font
face a small integer id and remembers the last style it resolved, so measuring and drawing a letter
no longer build a `String` to look one up.

`crates/quill-app/examples/frame_cost.rs` is how any of this is measured again: it opens a real file
with the real fonts of the machine, colours it as the window colours it, and prints what each part of
a frame costs. `tasks/task-1666-performance-tdd.md` has the before and after tables and the two
designs that were rejected.

## The Markdown preview draws pictures, and the layout engine knows nothing about pictures

`![alt](picture.png)` on a line of its own draws the picture, in the preview and in the right hand
pane of the side by side view. The preview is not a second renderer — `quill_core::markdown` turns
the source into the same three things a document holds and the ordinary layout and painter draw it —
so a picture had to become something the layout engine already understands.

It is `ParagraphStyle::min_height`: a paragraph may ask to be at least so tall, and layout takes
`(natural * line_spacing).max(min_height)`. A floor, never a ceiling. The line holding a picture is
**empty**, and the window paints into the room it reserved: the picture, or the alt text in the quiet
colour when the file cannot be read. An image mark **inside** a line of prose stays its alt text,
because a picture in the middle of a paragraph needs inline layout the engine does not have.

Two passes, and the reason is worth keeping: how tall a picture is drawn depends on how wide the pane
is and on how large the picture turns out to be, and `quill-core` can know neither — it has no window
and cannot decode an image. So `services::preview_images` reads the pictures, each asks its paragraph
for the room it needs, and only then is the preview laid out.

**Nothing is fetched.** A source with a scheme in it is refused rather than read, so a preview can
never make a network request. **And nothing is uploaded that the graphics card will not take**:
`services::picture::upload` shrinks a picture to the card's largest texture first, because egui
*panics* when handed a bigger one and a four thousand pixel screenshot is an ordinary thing to put in
a document. Both the picture tab and the preview go through it.

## A highlight is a colour behind a passage, and it moves with the text

Select some words, right click, choose one of four colours or open the colour wheel, and the
background behind those words is that colour — in this file, next time it is opened, and until it is
cleared. `task-1663` asks for it across as many files as you like, by hand or from the command line.

**The ranges live in `quill_core::highlights`, inside the `Document`.** A sparse set, sorted by where
each mark starts and never overlapping, which is what makes the one under the pointer a binary search
and the handful on the screen a binary search and a walk. It is **not** `StyleSpans`: that covers the
whole document with no gaps, so a document with two marked words would hold five spans; it carries no
alpha, deliberately, because text in Quill is always fully opaque; and formatting is inherited by
whatever is typed next, which a mark drawn over a passage must not be. Adding a mark **cuts away**
whatever it lands on, because two translucent colours over one another give a third colour nobody
chose and `Clear Highlight` under the pointer would have no single answer.

It lives in the document so that `insert` and `remove_range` — the only two places in Quill that know
a range of bytes moved — shift it in the same two lines that already shift `chars`. It rides the undo
`Snapshot` for the same reason everything else does: undo restores a state. **Marking is not an
edit**: it bumps the revision so the window repaints, and it does not set `modified` and does not push
an undo step, which is the rule the editor's font already follows.

**`services::file_marks` is the project's copy**, one `.quill/highlights.txt` for the whole project
beside `open-files.txt` — `<start> <end> <#rrggbbaa> <path>`, the path last so a path with spaces
needs no quoting, and relative so a project that moves keeps its marks. One file rather than one per
source file, because six hundred source files would be six hundred files to open when a project
opens. Read once and written only when something changed, and **only by the released binary**, exactly
as the project state is.

**One rule decides every awkward case: a file that is open is owned by its `Document`, and every other
file is owned by `FileMarks`.** The window pushes an open document's marks into the store when that
document's revision moves — an integer comparison a tab a frame — and pulls them out again when a file
is opened. `QuillApp::change_highlights` is the one place that choice is made, so no command has to
think about it.

**The right click menu is `components::text_menu`**, the editing area's first. The colour wheel is
drawn **inside the same popup** rather than in a second one over it, because egui keeps at most one
popup open at a time — the same rule that turned the three line spacings into three buttons. A right
click inside a selection leaves the selection alone; anywhere else it puts the caret there first,
which is what makes `Clear Highlight` mean the one under the pointer.

`components::color_wheel` is drawn rather than borrowed: egui's own picker is a square with two
strips beside it, and the ask is a wheel. The four colours are in `theme::color` and are accents
already sampled from the design; a colour chosen in the wheel is somebody's own mark on their own
text, which is the exception `design/style-guide.md` records beside a syntax theme's token colours.

`tasks/task-1663-highlights-tdd.md` records what was weighed. `quill-cli highlight` is the command
line half — `list`, `add`, `clear` and `apply`, the last taking a JSON array so twenty passages across
twenty files are one request and none of the files has to be opened.

## The editing area is a row of panes, and a pane is a number written on the tab

`task-1664` asks for IntelliJ's split view. Right click a tab, choose `Split Right`, and the editing
area is cut into panes side by side, each with **its own tabs**, its own scroll position, its own
view mode and its own gutter. The same entries are on `View -> Split` and in `quill-cli pane`.

**A tab moves into the new pane rather than being copied**, which is the one place Quill and IntelliJ
differ and is a decision Jason confirmed on the ticket. IntelliJ's `Split Right` shows the same file
in both splits; Quill cannot, because `OpenFiles::open` has always said that a file already open is
*shown* rather than opened twice — two tabs on one file would be two `Document`s over one path, and
whichever was saved second would win. This is IntelliJ's own `Split and Move Right` under the name a
person looks for. The exception is a pane holding **one** tab: taking its only tab away would empty
the pane it came from and leave the window looking exactly as it did, so there the tab stays and the
new pane opens empty and focused — which is what a person means by putting a pane on the right,
because opening a file always lands in the pane with the keyboard.

**Which pane a tab is in is written on the tab**, as `OpenFile::pane`, and which tab is showing in a
pane is the highest `OpenFile::shown_at` stamp in it. Neither is a list of indices, because every
index into `files` shifts when a tab is opened or closed and all seven pane operations would have to
fix them up. `OpenFiles::active_index` is then *the highest stamp in the focused pane*, which is why
the hundred and seven places that say `files.active()` did not have to change: "the file that is
showing" still has exactly one answer. Two invariants are kept by `OpenFiles::tidy` after every
change and asserted in the tests — panes numbered `0..panes` with no gaps, and no pane empty.

**A row of panes, not a tree of splitters.** There is no `Split Down` and no nesting.
`tasks/task-1664-split-view-tdd.md` §4 states what a tree would cost and what would have to change
for it; a row answers the ask and every operation on it is a small function with a unit test.

**What was laid out belongs to the tab, not to the window.** Ten fields moved from `QuillApp` onto
`OpenFile::cached` — the layout, its revision and width, the preview, its layout, its pictures and its
diagrams. With two panes at two widths a single cache is not slow so much as *wrong* in the way a
cache is wrong: the first pane lays its file out, the second lays its own over the top, and the next
frame does it again for ever. `coloured_revision` moved for the same reason, so the file in the second
pane is coloured too. Three caches deliberately stay on the window — `preview_images`,
`mermaid_scenes` and `icons` — because none of them is keyed on a document.

**The pane loop borrows the focus.** For each pane in turn the window sets `files.focus` to it, draws
the strip and the editing area exactly as it drew the single one, and puts the focus back at the end,
so `active()` answers with that pane's file for the duration and nothing had to have a pane index
threaded through it. Two things must **not** follow the borrowed focus and are passed in: the
keyboard, or every pane would take the same key presses and draw a caret, and `editor_area`, which the
status bar reads on the frame after. Everything in a pane is drawn into a `Ui` carrying the pane's
number as its **id salt**, because egui identifies a widget by its id and two gutters, or two
previews, would otherwise be one widget. The dividers are added after every pane, for the reason
`components::splitter` already records.

## The explorer follows the tab

The file showing in the pane with the keyboard is selected in the explorer, the folders above it are
opened out, and the list is scrolled the **least** amount that brings the row into view — `task-1664`
again. The pill was already drawn; what was missing is that a row inside a closed folder is not drawn
at all, a row below the fold is drawn where nobody can see it, and nothing noticed when the answer
changed.

`QuillApp::follow_the_open_file` is one rule for all three: it remembers the path it last revealed and,
when the file showing is not that one, calls `FileTree::expand` and asks the explorer to scroll. It is
**derived from the state rather than fired from each of the eleven places a tab can change**, because
the twelfth, added next month, would be the one that forgot. It is a **one shot** — a person who shut
the folder holding the open file shut it deliberately, and a reveal that ran every frame would open it
again before the pointer was up. `scroll_to_rect(row, None)` is what "the least amount" means, and it
is the same call `Go to File` and `Find in Files` already make. `View -> Select Opened File` and
`quill-cli explorer select-open-file` ask for it by hand.

## Git runs the `git` program, on a thread

`quill-git` shells out to `git` rather than using a library, and the reason is what the machine's own
git already knows: a credential helper, an ssh agent, `commit.gpgsign`, hooks, `safe.directory`, an
identity for this repository in particular. A push from Quill has to be the same push you get in the
terminal. The cost is that the output has to be read, which is answered by asking for the formats git
provides for being read — `--porcelain=v2 -z`, `--line-porcelain`, `--format` with the record
separators — never the ones meant for a person.

Two rules follow, and a change should keep both.

**Nothing invents an error message.** Every call returns git's own standard output and standard error
whether it worked or not, and that is what the status bar shows. A rejected push, a merge conflict, a
detached HEAD and a missing upstream all explain themselves better than Quill could.

**Every command goes through `quill_git::Worker`**, which runs one at a time on a thread. Not because
the window would be slow — because it would stop drawing until git finished, which on a fetch looks
exactly like a crash. One at a time, because two commands at once in one repository fight over
`index.lock`.

## A control is absent when it cannot apply, not dimmed

The `F` button is not drawn for a `.rs` file or a picture, and the three view mode buttons are not
drawn for a `.txt` one. Quill saves plain text and carries no formatting to disk, so everything behind
that button is about how prose is *shown*, and for a source file it is fourteen controls that mean
nothing — the three view modes offering the Markdown parser's reading of a file that was never
Markdown.

They used to sit in a strip of their own, forty four points tall, between the title bar and the tabs,
and the strip went with them: drawn for a `.md` file and not for a `.rs` one, it moved the tabs, the
explorer and the whole editing area up and down by forty four points every time the tab changed. A
control that appears is fine; a window that jumps is not. `task-1658` moved them to the right hand end
of the title bar, whose height never changes, and `components::text_tools` is what draws them.

Two functions in `services::file_kind` answer it, and nothing else does: `formatting_applies` and
`preview_applies`. The tools and the `View` menu both ask them, so they cannot come to different
answers about the same file, and a file kind stays decided in one place. `is_image` is the third
question of the same shape, and it decides whether a tab holds a picture rather than text.

Dimming means something different and is still right where it was: a control that could be used in a
moment, such as undo with nothing yet to undo, or the whole Git menu outside a repository. A control
that can never apply to this file is absent.

## The editor's font is one setting, and it reaches every tab

The family and the size in `Edit -> Settings -> Appearance -> Font` are one setting for the whole
window, the way IntelliJ has one editor font. `QuillApp::set_the_font_everywhere` is what puts it
into effect, and every path that changes it goes through that one function — the dialog, the
keyboard's command and plus, a trackpad pinch, and reading the settings file at startup. It used to
reach `document_mut()` alone, so opening three files and then changing the font left two of them in
the old one until Quill was restarted.

Setting it is **not an edit**: nothing goes onto any document's undo history and no file is marked as
having unsaved changes, because what Quill saves is plain text and carries no formatting.

**egui's own keyboard zoom is switched off**, in `theme::apply`. It scales the whole interface, menus
and all, which is a browser's zoom rather than an editor's; with it left on, one press of command and
plus would do both.

## Every pane is resized by dragging its edge

The explorer, the split between the Markdown source and its preview, and the terminal tile are all resized
by dragging, and they all go through `components/splitter.rs`. **A new pane must use it too.** The grab
width, the highlight while the pointer is over it, the pointer shape and the double click that puts the pane
back to its usual size are decided in that one file, so every divider in Quill behaves the same way.

Two things to know when adding one:

- The divider has to be added to the `Ui` **after** the panes either side of it. The editing area takes
  drags over the whole of its rectangle, and the divider overlaps its edge, so a divider added earlier sits
  underneath and never gets the drag. This was a real fault, found by a test that dragged and saw nothing
  move.
- Its size belongs in `settings::Panes`, which is written to the settings file, so the pane is where it was
  left next time Quill starts. Give it a smallest and a largest size and clamp both when reading the file
  and when dragging.

## A project remembers what was open in it, beside the project

`services::project_state` writes a `.quill` folder **inside the project** — beside `.idea` and
`.vscode` rather than in the person's own settings folder, so copying the project copies its state and
two people on one folder do not fight over one file. Three plain text files in the format
`services::store` already uses: `workspace.conf` for the flags, `open-files.txt` and
`expanded-folders.txt` for the two lists, one path a line. Paths are written relative to the project
wherever they are inside it, so a project that moves still opens the files it was left with.

Two rules that a change here must keep.

**Only the released binary reads or writes it.** `QuillApp::restore_project` is called from `main.rs`
and by nothing else, exactly as `load_settings` is. A test must not touch a person's files, and a
`.quill` written into a screenshot test's own sample project would change what the explorer draws in
the middle of a test.

**Terminals come back as fresh shells.** What a program was doing when the window closed cannot be
brought back; what is restored is the same number of shells in the project's folder.

## Everything is reachable from the command line, and that is enforced

`quill-cli` drives a **running** Quill: `quill-cli tab open README.md`, `quill-cli terminal send git
status`, `quill-cli settings set appearance.font.size 20`, `quill-cli window screenshot after.png`.
`task-1661` asks that every feature be reachable this way and be documented, and both are tests
rather than promises.

**A menu entry needs nothing at all.** `quill-cli action list` is built by walking the real menus, so
an entry added tomorrow can be run from the command line tomorrow. A test in `app/action_names.rs`
fails the day a menu entry has no name.

**Anything with no menu entry** is a row in `quill-cli/src/catalogue.rs` and an arm in
`app/cli.rs`. The catalogue is one list in a crate both halves depend on: the client parses against
it and the window dispatches on it, so a command the CLI accepts is a command the window knows.

**Documentation is a test.** `quill-cli/src/documentation.rs` fails while a command has no section in
`quill-cli/docs/commands.md`, while a section's usage line is out of date, or while a section
describes a command that no longer exists. `cargo run -p quill-cli --example reference` writes it. A
second test parses every example in the catalogue and checks it runs the command it is filed under,
because the examples are what an agent copies.

`QuillApp::run_cli` is to the command line what `run_action` is to the menus: **the one place a
command turns into a change**, and wherever there is already a way in it uses it. So a thing done
from the command line and the same thing done by hand are the same thing.

`services::control` is the channel: a thread on `127.0.0.1`, a port the operating system chose, one
JSON object a line, and a per-run token in an instance file under the person's own settings folder.
Loopback only, with a test for it. The window answers at the **top of a frame**, before anything is
drawn, which is what makes a screenshot taken straight after a command show what the command did.
`quill --control off` closes it. `tasks/quill-cli-tdd.md` records what was weighed;
`quill-cli/docs/protocol.md` is what a client in another language needs.

**How well it reads.** `quill-cli/agent-assessment/` measures it rather than assuming it: the local
Qwen 3.8 27B, given only `docs/commands.md`, carries out 64 instructions phrased as a person would
say them and scores **100%**, five rounds running, at two temperatures. The same 64 instructions with
the documentation withheld score **3.13%**, which is what makes the first number mean something.

## One action, one place

Everything a menu or a keyboard shortcut can ask for is an `app::actions::Action`, and
`QuillApp::run_action` is the only place an action turns into a change. There are two menu bars, the one
macOS draws along the top of the screen and the one Quill draws inside its own title bar on Windows, and
both are built from `app::actions::menus`. Adding an entry means adding a variant, an entry in that list and
an arm in `run_action`, and both bars get it.

Do not read the keyboard for something that is also a menu entry. On macOS a shortcut on a menu item is a
key equivalent, and AppKit hands it to the menu before the window sees it, so the key press never reaches
egui. Cut, copy and paste are the exception, marked in the list as not coming from the keyboard, because
the platform delivers those as clipboard events.

## Components take a rectangle and return what happened

A component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and returns what
the user did in it. It does not change the document or the window's state. The state changes in `app`, so
two components cannot disagree about what happened, and a component can be drawn by a test without a
document behind it.

Everything is painted at an absolute position rather than through egui's layout, because the window follows
`design/intial-design-screenshot.png` and the measurements come from that image. `theme::size` holds them.

## Give every control a name

Every control calls `response.widget_info` with a plain name: `Save`, `Bold`, `Resize explorer`,
`Terminal tab: claude`. The screenshot tests find controls by name rather than by position, so a control
that moves does not break a test, and a control with no name cannot be tested at all. Two controls must not
share a name: the Settings window's button says `Done` rather than `Close` because the window already has a
`Close` button.

## The desktop shows through, and Windows needs three things for it that macOS does not

The window is created with `with_transparent(true)`, the background is painted with the opacity
setting's alpha and every glyph is painted at alpha 255. On macOS that is the whole story.

On Windows the same code drew a solid window, and `services/windows_transparency.rs` is what fixes
it. Three separate faults, each of which alone is enough to leave the window opaque: wgpu picked
Vulkan, whose surface offers no transparent composite mode, so DX12 is named; a DXGI swapchain built
from a window handle can only be `Opaque`, so it is built from a DirectComposition visual instead;
and the window's redirection surface is never cleared by winit, so it holds undefined bytes that
composite as opaque white, and it is filled with black — which is how GDI writes a zero alpha — once
a frame. **Filling it once does not work**, because eframe keeps the window hidden until it has
painted its first frame, so the fill lands before Windows has allocated the surface. Section 9.2 of
`tasks/quill-technical-design-document.md` records how each was measured and what was rejected;
`design/verification/live-window-over-desktop-windows.png` is what it should look like.

## Shipping it is a folder of its own

`installer/` turns the built binary into something a person can install, and it does not reach into the
application: it packages what `cargo build --release` already produces. `installer/icon` draws the mark
once and writes `quill.ico`, `quill.icns` and an iconset; `installer/windows` compiles an Inno Setup
script into a single setup exe; `installer/macos` builds `Quill.app` and a disk image. Each has a build
script that goes from a checkout to a file that can be handed to somebody, so nothing about releasing
lives only in a person's memory.

The one place it does reach in is `crates/quill-app/build.rs`, which puts the icon and a version block
inside `quill.exe`. That has to be inside the executable — Windows reads the taskbar icon, the Alt-Tab
entry and the Add or Remove Programs version from there, and no installer can supply them from the
outside. A missing Windows SDK makes it a warning rather than an error, so a build with no `rc.exe`
still produces a working, if unlabelled, `quill.exe`.

**The version lives in `Cargo.toml` and nowhere else.** It reaches the resource block, the installer's
file name, the Add or Remove Programs entry and `Info.plist` from there. Do not write it down a second
time.

## Tests

Four layers, and a change should leave all four green:

1. `quill-core` and `quill-terminal`: unit tests with no window. Layout tests measure through a fixed width
   stub, so the expected numbers are arithmetic a reader can check and are the same on every machine.
2. `quill-app`: unit tests for the file tree, the fonts, the settings file, the menus and the key encoding.
3. `crates/quill-app/tests/screenshots.rs`: builds the whole window through `egui_kittest`, feeds it real
   events, renders through `wgpu` and writes a PNG for each test. **Look at the images.** They are how a
   person or an agent confirms that bold text is bolder and that the terminal's colours are right. Once
   accepted they are the comparison baseline, so a later change that alters the rendering fails a test.
   `UPDATE_SNAPSHOTS=1 cargo test` accepts new images, and nothing should be accepted without opening it.
   Each platform has its own accepted set — macOS reads `tests/snapshots`, Windows `tests/snapshots/windows`
   — because the menus, the window buttons and the font are all deliberately different there, so one set
   cannot be the baseline for both. `shot()` at the top of the test file is where that is decided.
4. The real application: `cargo run --release`, and `cargo run --example terminal_capture -- claude` for the
   terminal. For git, `pwsh tools/build-git-demo.ps1` builds a small repository under the temporary
   folder — three commits by three authors on three widely separated dates, a branch, an uncommitted
   change and an untracked file — which is enough to exercise every entry on the Git menu by hand. Layer 3 renders through the same code but offscreen, so only a real run shows that the
   operating system honoured the window's transparency or drew the menu bar.

**A performance change is measured, not asserted.**
`cargo run --release -p quill-app --example frame_cost -- <file> [width]` lays a real file out with
the real fonts of this machine, colours it as the window colours it, and prints what each part of a
frame costs. It is not a test and nothing fails it: a threshold in milliseconds would be a different
number on every machine. What *is* a test is the work itself — how many glyphs the painter placed,
how many clusters the fonts were asked to measure — because those are the same on every machine.
`tasks/task-1666-performance-tdd.md` §11 lists the five that fail on the code as it was.

A screenshot test must be the same on every run. The terminal's screenshot tests feed fixed bytes to a
session with no shell behind it, through `QuillApp::new_detached_terminal_tab`, because when a real shell
answers is not something a test can know. Tests that do run a shell assert on text and wait with a timeout.

Three rules the screenshot tests follow, all of them the answer to a test failing for a reason that was
not a fault in Quill (`task-1654` — the file's own comments carry the detail):

- **Nothing builds a graphics device of its own.** `egui_kittest`'s `.wgpu()` builds a new instance,
  adapter and device for each harness, and ninety one of those built and torn down across as many
  threads as the machine has killed the process with an access violation on about one run in nine.
  A small pool is built once at first use and shared, and every harness comes from `builder()` in the
  test file rather than `Harness::builder()`, so a test added later cannot go back to one of its own.
- **A fixture two tests share is written once.** `sample_folder()` used to write its files on every
  call, so one test could read a file another test had truncated a moment ago and not yet filled in.
  It is built behind a `OnceLock` now. A fixture only one test uses, like `git_folder(name)`, may be
  written each time — the name is what keeps them apart.
- **A loop that waits calls `pump`, not `Harness::run`.** `run` gives the window four steps to go quiet
  and panics otherwise, which is right for a settled window and wrong while git or an image is still
  being worked on. Running out of steps inside one attempt is not a failure; running out of attempts
  is, and the loop says so.

Tests must not read or write the settings of the person running them. `QuillApp::new` reads nothing; the
released binary calls `load_settings` and a test that wants a store calls `use_store` with a folder of its
own.

## Writing

Plain sentences. Say what the code does and why a decision was made, once. Every module has a comment at
the top saying what it is for, and a decision that a reader might disagree with is recorded where it was
made rather than left to be rediscovered. There are examples throughout: why undo restores a saved state
instead of an inverse, why the terminal's lock is held for the copy and not for the drawing, why a second
Quill window is a second process.

British spelling in prose, and the American spelling where a name in the code already uses it, such as
`color` in `egui`.

## Plugins are data, and nothing in one is executed

A plugin is a folder with a `plugin.conf` and an icon in it, read by the same value store the
settings file uses. It describes a language: its extensions, its keywords, what a comment and a
string look like, and a colour per kind of token. **Nothing is run.** A dynamic library would mean an
unstable interface across a `dlopen` boundary and a plugin crash taking the editor with it;
WebAssembly answers both and costs a runtime and a host interface that has to be designed first.

`plugin.kind` is the seam a later version widens, and it is checked: a manifest asking for anything
but `language` is refused with a message rather than half-loaded. Do not quietly widen it.

The Mermaid plugin did **not** widen it. Mermaid is a language — keywords, comments, strings, an
extension — so it is an ordinary `language` plugin, and it carries one new data key,
`language.renders = mermaid`, naming a renderer that is **built into Quill**. Nothing is loaded from
the plugin; the manifest says "files of this language have a picture, and this is which picture", and
the code that draws it shipped with the binary. The value is checked against `plugins::RENDERERS` and
a manifest naming one this version does not have is refused with a message, exactly as `plugin.kind`
is. What it buys is that switching the plugin off actually withdraws the feature: the window asks
`Plugins::renders` before it draws a diagram anywhere, so `.mmd` files stop being drawn and mermaid
fences go back to being code in the same frame.

A bundled plugin's icon is generated rather than drawn, and each one records how:
`plugins/mermaid/icon.md` has the prompt, the endpoint and the two commands, so it can be made again
without guessing.

A colour scheme **colours the tokens and not the editing area**. The window letting the desktop show
through is the whole character of the product, and a scheme that repaints the background opaque would
trade that away to be a shade nearer a screenshot.

## The documents

- `README.md` — what Quill is and how to run it.
- `documentation/overview.md` — what Quill looks like: a capture of each part of the window, over a real desktop.
- `design/style-guide.md` — how a control in Quill is built, and what the baselines are.
- `tasks/quill-ide-tdd.md` — the line numbers, the tabs, the explorer's menu, git and the plugins:
  what was chosen, what was rejected and why.
- `tasks/quill-technical-design-document.md` — the editor: the options that were considered, what was
  chosen and why, and what is deliberately not included.
- `tasks/quill-terminal-tdd.md` — the same for the terminal.
- `tasks/task-1657-text-options-tdd.md` — the `F` button and its flyout, the font becoming one
  setting for the window, zooming with a pinch and with the keyboard, and the two drawing faults
  that were fixed alongside them.
- `tasks/task-1658-window-improvements-tdd.md` — the window's own resize grips, the rail of pane
  buttons, the `.quill` folder a project remembers itself in, the tools moving into the title bar,
  and pictures opening in a tab.
- `tasks/task-1659-search-and-images-tdd.md` — `Go to File`, `Find in Files` and the thread it reads
  the project on, modals that can be dragged and resized, and pictures in the Markdown preview.
- `tasks/task-1664-split-view-tdd.md` — the explorer following the tab, and the editing area split
  into panes: why a tab moves rather than being copied, why a pane is a number on the tab, why the
  layout caches had to move onto it, and what a tree of splitters would have cost.
- `tasks/task-1663-highlights-tdd.md` — highlighting a passage: where the ranges live so they move
  with the text, the file beside the project that remembers them, the right click menu and the drawn
  colour wheel, and the bulk commands.
- `tasks/task-1666-performance-tdd.md` — why a frame cost 818 ms and now costs 20: the eight faults
  that were found, what each was worth, the two revisions a document counts, the incremental layout
  and why its fingerprint is derived rather than reported, and what was deliberately not done.
- `tasks/quill-mermaid-plugin-tdd.md` — Mermaid: the four ways of drawing it that were weighed and why
  Quill writes its own, what each of the twenty types becomes on the screen, which ten are named
  rather than drawn, and what `language.renders` buys.
- `tasks/quill-cli-tdd.md` — the command line: the transports that were weighed, the command surface,
  the wire format, what the token is and is not worth, and how the 97% was to be measured.
- `quill-cli/docs/commands.md` — **the reference, written to be handed to an AI agent whole.**
- `quill-cli/docs/protocol.md` — the socket underneath, for a client in another language.
- `quill-cli/agent-assessment/qwen-38-27B-assessment.md` — how well a local model does with it,
  measured against a live window.
- `tasks/quill-installer-tdd.md` — how Quill is delivered: the icon, the Windows installer and the
  macOS bundle, and the options that were weighed for each.
- `installer/README.md` — how to build an installer, on either platform.
- `tasks/improvements.md` — the ask that the settings window, the panes, the terminal and the menus came
  from.

Each document stands on its own. If a fact from another one is needed, state the fact.
