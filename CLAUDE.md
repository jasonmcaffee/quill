# Working on Quill

Read this before changing anything. It records the conventions the code already follows, so that a later
change looks like the rest of the code rather than like a second style laid over it.

## Finishing a task means releasing it

**When the work is done and verified, run `pwsh tools/release.ps1`.** Patch by default, `-Part minor`
for a feature. It bumps the version in `Cargo.toml`, rebuilds — which is what moves the build date
the About box shows — reinstalls Quill on this machine, tags `v<version>`, pushes, and publishes the
GitHub release with the installer attached. Commit the task's own work first; the script refuses to
run on a dirty checkout, and the version bump is a commit of its own so the history stays greppable
by ticket.

This is not paperwork. `Quill -> About Quill` is how a person answers "is this the build with the fix
in it", and it can only answer that if the build they are running is the build that was just made. A
task whose change is still sitting in `target/release` is a task that was not finished: the editor on
the desktop is the old one, and the version on the About box says a number that no longer means
anything.

`tools/release.ps1 -WhatIf` says what it would do and changes nothing. It installs `gh` with winget
the first time, exactly as the Windows installer script installs Inno Setup, and takes the GitHub
token from the credential helper git already uses, so there is no second credential to keep.

## What the crates are for

| Crate | What is in it | What must never be in it |
|---|---|---|
| `quill-core` | The editor: the text buffer, the character and paragraph formatting, the caret, layout, undo, the Markdown parser, the syntax tokeniser, and the Mermaid reader and diagram layout. | Any user interface dependency. Its tests run with no window, no graphics card and no fonts. |
| `quill-terminal` | The terminal: the session over a pseudoterminal, the screen the painter reads, the colour palette, the key encoding, the mouse reports, and which shell to start and in what folder. | Any user interface dependency, for the same reason. |
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

**The Settings window is one size for every page, and the tallest page is what it has to hold.** It
grew from 560 to 640 points when `task-1679` added the MCP page, because a dialog that changed height
as its list was walked would jump under the pointer, and because a settings page here does not scroll.
The other four gained empty space, which is the cheaper of the two costs. If a page ever needs more
than 640, that is the point at which a scrolling page area is worth building rather than another
eighty points.

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

## Where a name is defined is read from the tokeniser, and nothing is executed to find out

`Ctrl/Cmd+Click` goes to a definition, `Alt+F7` lists every reference, and `Shift+F6` renames one
everywhere it is used. `task-1675` weighed the three mechanisms every editor sits on and chose the
third: a **syntactic index**, built from the token stream `quill_core::syntax` already produces.
A language server client would be the true answer and would die on most machines — a separate
program per language, found on `PATH`, holding gigabytes, and nothing about it could be a screenshot
test because when it answers depends on the machine. Tree-sitter is code where Quill's plugins are
data. This is the tier Sublime Text's goto-definition and GitHub's shipped code navigation are, and
it is what makes the answer instant, deterministic and testable with no window.

**What a definition is comes from the plugin, not from a list of languages in Quill.** Two manifest
keys, both off unless a language asks for them, which is the rule `word_characters`, `types` and
`hex_colors` already follow: `language.definers` is a comma list of `keyword=kind` — `fn=function,
struct=type, let=variable` — and `language.brace_definitions` turns on the one heuristic, for the
definition Rust never hides but JavaScript and TypeScript do: a class method has no keyword in front
of its name. CSS deliberately names neither. `--brand-hue: 280` defines a custom property by
*position*, and a rule that read `:` as a definer would call every property a definition; find all
references and rename still work there, because neither needs a definition.

**Honesty is the whole of the design.** Where the mechanism cannot tell two same-named things apart
it shows both rather than guessing one, a definition found by the brace heuristic is marked
`Confidence::Likely` and stays marked all the way to the screen, and an occurrence inside a comment
or a string carries the `Role` that says which — listed second, in the quiet colour, and never
ticked by default in a rename.

**One pass, read once and asked many times.** `quill_core::syntax::scan` is the seam: it reports
every token including the plain words, and `highlight` is that with the plain words dropped, so
colouring a file and reading its definitions are one reading of the rules rather than two that
would drift. `symbols::FileSymbols::read` turns one file into two sorted lists — its words and its
comments and strings — and the window keeps one per open tab keyed on `Document::text_revision()`,
the same key `colour_the_file` is keyed on. That is what makes the hover query, which runs while the
pointer moves with the modifier held, a binary search rather than a re-read: measured on this
repository, reading the largest file costs 1.8 ms and a hover costs nothing measurable.

**Definitions are indexed and occurrences are not.** `services::symbol_index` holds `name -> where
it is defined` for the project, built on a worker thread arranged exactly as `services::text_search`
is — a generation `AtomicU64`, a waker, and a build that stops where it is when it is overtaken.
Find all references is a **search** instead, in a new whole-word, role-classified mode of the same
searcher: an index of every occurrence would buy nothing at this size and would cost the one thing a
search never pays, which is invalidation — a build, a branch switch or another editor moving a file
would all have to be noticed. `examples/symbol_cost.rs` measures the lot: 155 files indexed in 38 ms,
11,497 definitions, a reference search over 176 files in 42 ms.

**One rule settles every awkward case, and it is the highlights' rule**: *a file that is open is
owned by its `Document`, and every other file is owned by the index.* An open tab's definitions come
from its live text, the index's copy of an open file is never offered beside it, and a reference
search is handed the text of the tabs rather than the bytes under them. The disk-owned side is
**re-checked at the moment of use** rather than watched: before jumping into a closed file its text
is read again and the name confirmed to still be there, re-found if it moved, which is what
`open_the_match` already does for a search hit.

**The rename modal is the preview, and the tick boxes are the change set.** What is applied is
exactly the ticked rows. An open file is edited as a document — one `Command::ReplaceMany`, which is
one undo step by construction because undo restores a snapshot — and is left with unsaved changes
rather than being written, because a rename must never silently write a buffer somebody was editing.
A closed file is read, **every ticked range is checked to still hold the old name**, and only then is
it written once; a file that changed since the search is skipped whole and reported by name rather
than patched on faith. Bytes outside the ranges are untouched, so encodings and line endings survive,
and `services::file_marks` shifts that file's stored marks by the same edits — a rename is the one
new place a closed file's bytes move. A collision is a **warning**, not a refusal: the mechanism
cannot know whether it shadows, so it says what it does know.

**The three entries are absent when the file's language cannot answer them**, which is Quill's rule
for a control that can never apply — the `F` button is not drawn for a `.rs` file either.
`file_kind::definitions_apply` and `file_kind::symbols_apply` are the two questions, and the Edit
menu, the editing area's right click menu and the command line all ask them, so none of the three can
disagree. That absence is also what lets the command key and `B` mean bold in prose and `Go to
Definition` in code: `formatting_applies` and `definitions_apply` are true of opposite files, so the
two can never both fire on one press.

## A name is offered while it is still being typed, and nothing new is indexed to do it

Type two letters of a word in a file a plugin claims and a list of the names it could become appears
under the caret; `Up` and `Down` steer it, `Tab` and `Enter` take a row, `Escape` puts it away, and
typing carries on underneath it the whole time. `Ctrl+Space` and `Edit -> Complete Word` ask for the
same list by hand. `task-1677` is the design and `task-1678` is the implementation.

**Everything it offers was already in memory**, which is what makes it a small feature where most
editors' completion is an enormous one. Four sources, all of them kept fresh by `task-1676`: this
tab's definitions and its **distinct words**, cached on the tab and keyed on
`Document::text_revision()`; the other open tabs' definitions; `services::symbol_index`, which gained
a sorted name list to scan; and the file's `Grammar`, whose keywords, builtins and types the manifest
has held since the plugin was read. No new thread, no new index, no watcher, no debounce. The
ownership rule carries over unchanged: the open files' paths are dropped from what the index offers,
or a name being edited in a tab would be offered twice, once live and once as the disk last saw it.

**The match is a case-insensitive subsequence and the score is Sublime's rubric** — a large bonus for
a prefix, one per matched letter on a word boundary, one per consecutive letter, a small one for
matching case, and a penalty per unmatched letter so the shorter of two names wins. The alignment is
the **best** one rather than the first, found by filling a small table, because `pt` reads
`paint_text` two ways and only one of them is the one a person meant. `quill_core::completion` is all
of it, pure, tested with no window — and **its tests pin orderings, never scores**: a test asserting
`-13` would be a test of the constants rather than of anything anybody can see.

**The row equal to the stem is never offered**, and that one rule is what makes `Enter` safe. VS Code
grew a three-way setting (`editor.acceptSuggestionOnEnter`) because people pressed `Enter` meaning
"new line" and got a suggestion; dropping the no-op row answers it at candidate time instead, so once
a word is completely typed the list has either something genuinely longer to offer or nothing at all
— and with nothing to offer it has already closed.

**Exactly five keys are consumed, and only while it is open**: `Up`, `Down`, `Tab`, `Enter`,
`Escape`, taken out of the frame's input before any pane reads it, which is the one-frame ordering
`Find in Files` and `Go to File` already rely on. They are taken with the modifiers compared **for
real** rather than through `InputState::consume_key`, which matches by `Modifiers::matches_logically`
— that only asks whether the modifiers the *pattern* names are held, so a pattern of `NONE` matched
`Shift+Enter` as well and the popup was swallowing an ordinary new line. `Ctrl+Tab` is still
`Next Tab`, and the three meanings of `Tab` — move tab, indent, complete — stay on three distinct
chords. `Tab` replaces the whole identifier and `Enter` replaces the stem, which is IntelliJ's own
distinction and is right in both directions.

**The popup is an `egui::Area`, not an `egui::Popup`.** egui keeps one popup open at a time — the
rule that already shaped the flyouts and the colour wheel — and this list must coexist with nothing
*and* must never take the keyboard. It is drawn after the pane loop from the caret geometry the pane
with the keyboard recorded, so it is never under a divider and never drawn twice in a split view.

**Nothing here runs once a frame.** The state carries the `text_revision` and the caret its rows were
worked out at, and a frame in which neither moved costs two integer comparisons — `task-1666`'s rule,
kept the way `symbols::Hover` keeps it. The keystroke budget is **under 5 ms on the largest file in
this repository**, and `cargo run --release -p quill-app --example completion_cost` is how it is
measured again. It was over that when first written (6.6 ms), and the fix was removing waste rather
than capping the pool: the dedup was a linear search over the rows already kept, the alignment table
and the candidate's characters were three allocations *per candidate*, and the sort comparator counted
every name's characters at every comparison. 4.59 ms when that was written; **5.06 ms today, and the
number to read is 4.42** — the file it is measured against is `tests/screenshots.rs`, which has grown
to 271 KB, and the 5.06 is a *one* character stem, which only `Ctrl+Space` can ask for because the
popup does not open under two. Nothing in `task-1680` moved it: the export marker it added to
`FileSymbols::read` measures 0.000 ms, and the example prints that line so it stays measured rather
than assumed.

`editor.suggestions` is `automatic` or `manual`, with a tick box in `Settings -> Editor`. `manual` is
already the off switch — `Ctrl+Space` and the menu entry work either way — which is why there is no
third value. `quill-cli editor complete` prints the rows and `--choose` applies one, both through the
same functions the popup uses.

## Inside an import, the list is the files, and what they export

Start writing `import { } from '` and the list under the caret is the project's own files; put the
caret between the braces and it is what that file exports. `use quill_core::comp` offers the same
thing walked down a module tree instead. `task-1680` is the design, and it is one new question asked
before `task-1678`'s four sources are gathered: *is the caret in the middle of an import?* When the
answer is yes the four sources are **not** gathered at all, because a keyword, a local word and an
unrelated name from the project are all wrong answers to `from '│'`.

**Two families, because there are two shapes and no third.** A `quoted` module is a string resolved
against the file system — TypeScript, JavaScript, CSS — and a `path` module is segments resolved
against a module tree, which is Rust's `use`. `quill_core::imports::context_at` reads which, working
**backwards from the caret** for the reason `symbols::identifier_at` does: a few hundred bytes of
scanning a keystroke rather than a reading of the file, and a half-typed line above cannot poison the
answer. The path family's walk **is** its parse, and the keyword it ends at is the whole of what
makes it trustworthy — `use` in front and it is an import, anything else and `a::b::c` is ordinary
code.

**The tier is syntactic and says so.** No language server: `task-1675` §2 already weighed and
rejected one, and every reason holds here. The precedent for this tier is Vim's `Ctrl+X Ctrl+F`,
which completes a file name off the disk with no language knowledge at all, and VS Code's built-in
path completion for HTML and CSS. What is offered is what is really there — the files come from
`FileTree::all_files`, the same list `Go to File` searches, so a specifier Quill offers is one that
really resolves and nothing outside the project can be reached.

**Nine manifest keys, every one off unless a language asks**, which is the rule `language.definers`
set: `imports`, `import_keywords`, `import_extensions`, `import_index`, `import_omit_extension`,
`export_keyword`, `path_separator`, `source_roots` and `path_roots`. The alternative was a list of
languages inside Quill, which is the exact thing those keys exist to prevent and which would mean a
plugin somebody writes for Python could never have the feature.
`the_older_plugins_ask_for_none_of_what_the_imports_added` pins that Mermaid is unchanged by any of
it, and a manifest naming a shape or a root this version does not have is refused with a message,
exactly as `plugin.kind` and `language.renders` are.

**The inserted specifier is always relative.** VS Code makes its shape a setting with four values
because there is no single right spelling of one; a relative specifier is the one that is *always*
right, needing no `tsconfig.json`, no `baseUrl`, no alias table and no `exports` map.

**`language.export_keyword` is what makes a definition importable** — `export`, `pub` — and it is
`quill_core::symbols`' answer, not a second reading: the marker reaches its declaration along one
line, over `default` and over `pub(crate)`, and `export { a, b }` marks what it names wherever that
was declared. A language naming none hides nothing. The ownership rule of `task-1675` §3.3 decides
where a module's exports come from, unchanged: a module that is **open** is owned by its `Document`,
so a function added in the tab beside this one is offered before it is saved, and every other module
is owned by `services::symbol_index`, which gained one small table of each file's exported names so a
closed module answers without being read again on every keystroke.

**Nothing new is indexed and no thread is added**, which is the same sentence `task-1678` opens with.
The one unbounded moment is a specifier with nothing typed, which turns every importable file into a
row: it happens once per import statement, and the next character makes `completion::could_match`
throw nearly all of them away before a candidate is built. Measured, it is 0.87 ms at worst.

Two smaller things it changed, both of which had to change. `completion::rank_all` sits beside
`rank`: an empty stem offers **everything** rather than nothing, because `from '│'` and `use │` are
positions at which the language itself says what comes next, which is why IntelliJ opens its own
popup at zero characters after `import`. And `Command::ReplaceMany` now treats an **empty range
carrying text** as an insertion — accepting `./layout` inside `from '│'` replaces no bytes and
inserts all of them, and it has to be the same one-undo-step command every other completion is. An
empty range carrying nothing is still dropped, because it would be an undo step for an edit that
changes no byte.

Deliberately not here, each with its reason in §12 of the TDD: **auto-import** (a different feature
with a different risk, since it edits a part of the file the caret is not in), **bare package
specifiers** (`node_modules` is out of the walk and `task-1659` measured what putting it back would
cost), **`tsconfig` path aliases**, and **following re-exports**.

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

## The Markdown preview is a document, which is what makes it read like one

`task-1685` reported four things: tables were not drawn, the preview could not be selected or copied,
code blocks were hard to read, and "a lot of formatting isn't complete or is missing". The fourth is
the one that mattered — the old parser read the source **one line at a time**, decided what that line
was in isolation, and walked its characters with three booleans for bold, italic and struck. That
shape cannot express a list item holding a paragraph, a quote holding a list, a fence indented inside
a bullet, or the question of whether the `*` in `2 * 3 * 4` opens emphasis. Twenty-eight separate
omissions were downstream of it, and they are the table in §1 of
`tasks/task-1685-markdown-tdd.md`.

So `quill_core::markdown` is the two phases every conforming implementation uses.
**`blocks`** builds a tree, recursively: a quote's lines have one `>` taken off them and are parsed
again, a list item's lines have its indent taken off them and are parsed again. Two rules in it are
worth naming — **lazy continuation**, which is what makes hand-wrapped prose one paragraph instead of
five and is the single most visible fix in the ticket; and **tight and loose**, which is why lists
stopped looking so airy, since every item used to be followed by a blank line whatever the source
said. **`inline`** is CommonMark's **delimiter stack**: a run of `*`, `_` or `~` is measured, asked
whether it may open and whether it may close under the flanking rules, and matched against the runs
still open behind it. A run that can do neither is text, which is the whole of why `2 * 3 * 4` is now
left alone and why the special case for `snake_case` could be deleted.

**It is still not a second renderer, and that is the point.** The preview produces the same three
things a document holds, so the ordinary layout, the ordinary painter, the ordinary scrollbar and the
ordinary hit testing draw it — which is why **selecting text in it was a small feature**: the bytes
under the pointer are `Layout::offset_at`, the rectangles are `Layout::selection_rects_in`, and the
only new state is a `Selection` on the tab beside the scroll position. A `pulldown-cmark` was weighed
and refused for the reason `task-1675` refused a language server: its output is events shaped for
HTML, and Quill has no HTML, no box model and no inline layout, so everything it gave back would have
to be walked and re-expressed anyway.

**A table is set in the code font and drawn in a box.** §5 of the TDD weighs the three ways of giving
a glyph-placing layout engine a column. Tab stops on `ParagraphStyle` are the prettiest and cost the
most — it is `Copy` and twelve bytes, one per line of every document, and a cell that wraps makes one
visual line hold pieces of several cells while `PlacedLine::bytes` is a single `Range`. Drawing the
table as a picture is cheap and wrong, because a picture of a table is a table nobody can copy out
of. So every cell is padded with spaces to its column's width: the columns line up **by construction
rather than by measurement**, the arithmetic is integers over characters and every one of its tests
runs with no fonts, and the whole table is ordinary text — so it selects, copies and hit-tests with no
new code, and what lands on the clipboard is a table a person can paste anywhere. It is what
`glamour`, `rich`, `mdcat` and `bat` all do. The one measurement is how many characters of the code
font fit across the pane, and the caller takes it.

**A rule is drawn rather than lettered.** The box-drawing characters are in the text, but
`components::editor_view::box_rules` paints them: a glyph cannot tile, because its ink is an em box
while the line it sits on is taller, and its bitmap is a whole pixel wider than its advance. Set as
letters, a table's rules came out dotted and its columns came out as rows of ticks. Eleven characters,
each a pair of half-cell strokes. `design/style-guide.md` already said this about icons.

**Code is coloured by the plugin that reads the language.** The seam is `CodeHighlighter`, one method:
`quill-core` holds no plugin registry and must not learn about one, so it asks and the window answers
with the same `syntax::highlight` and the same theme `colour_the_file` uses for a source file.
`Plugins::for_language` matches a fence's word against a plugin's id, its name and every extension it
claims, so ```` ```rs ```` and ```` ```rust ```` are one question and Quill holds no table of aliases.
A language nothing claims falls back to the one code colour, which is what the preview did before, so
the change can never make anything worse. Where the code blocks are comes back as `Preview::panels`
and where the inline code is as `Preview::code_spans`, and the window paints a panel behind the first
and a chip behind the second — this crate says which paragraphs and which bytes, and the window
decides what a code background looks like.

**The preview never takes the keyboard.** In the side-by-side view the source is being typed into and
the preview is being read, and a click in the preview that stopped the caret working would be worse
than having no selection at all. So `Focus` is untouched and one flag, `QuillApp::reading_preview`,
says which of the two a copy is about: set by a press in a preview, cleared by a press in an editing
area. `Ctrl/Cmd+C` arrives as an `Event::Copy` rather than as a key press, so it is claimed **before
the pane loop**, which is the one-frame ordering the completion popup and the explorer's keys already
use — the source pane is drawn first and would otherwise copy its own selection. A selection is thrown
away when the preview is worked out again, because a byte range into text that has been rebuilt means
nothing.

**Four invariants hold for every preview**, checked for every case in the battery and by
`cargo run --example markdown_check`, which reads every `.md` in the checkout — 55 files, a megabyte,
116 ms — and reports any that break one: the spans cover the text exactly, there is one paragraph
style and one source line per preview line, the source lines never go backwards, and everything a
picture, a diagram or a panel names is inside the text. One of these failing is not a wrong-looking
document, it is a blank pane: the layout engine indexes the paragraph list by line number and the
scroll crossing indexes `source_lines` the same way.

`quill-cli editor preview-select` is the command line half, and `editor preview --json` reports the
panels and the inline code beside the text.

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

The explorer's heading is the project folder's row. It has no row of its own in the tree, so a right
click on the name opens the same menu a folder row opens — `task-1673`, which also took away the plus
beside it: it was labelled `New file` and it called `save`, which is why it never made one.

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

## The explorer holds the keyboard, and a file can be thrown away or moved

`task-1681` asks for three things that look unrelated and are not: a file can be deleted, a tab that
was edited saves itself when it is closed, and a file dragged to a new folder takes the code that
names it with it. Two of them needed something Quill had never had, and it is the same thing.

**`Focus` has a third value.** `Delete` cannot mean "throw this file away" while the editing area
has the keys, because there it means "take away the letter in front of the caret". So the explorer
holds the keyboard, has a **selection** of its own — separate from the file that is showing, because
they are different questions — and is walked with the arrow keys. `Enter` opens the selected file
permanently and hands the keyboard to the editor, `Escape` hands it back, and `Delete` asks the
question. The selection is drawn as the pill the open file already has, with a one point `ACCENT`
ring **only while the explorer has the keyboard**, so a person can always see where a key press is
going.

**A letter typed hands the keyboard straight to the editor**, and that one rule is what makes the
paragraph above safe rather than annoying. Clicking a file in the tree and then typing is one of the
commonest things anybody does; the explorer has no use for a printable character — its filter box is
a field of its own — so any letter means the editor, and the handover happens *before any pane reads
the frame's input*, which is what makes the letter that caused it land in the document. `Delete` and
the arrow keys produce no text event and stay the explorer's. **`Backspace` on its own deliberately
does nothing**: it is exactly the key somebody who has just clicked a file is about to press in the
editor. The command key with it is IntelliJ's own answer for the Mac keyboard that has no `Delete`.

**Where a deleted file goes is one function**, `services::recycle`. On Windows it is the Recycle Bin,
through `SHFileOperationW` with `FOF_ALLOWUNDO` — one feature flag on the `windows-sys` dependency
the window's transparency already needs, and no new crate. Everywhere else it is deleted outright,
and the question says so: `Destination` is an enum with two values and the dialog's wording is
derived from it, so the day the second platform is answered there is one place to change and the
sentence follows. `Confirmation` stopped holding a git request and started holding an `Answer`, so
the one confirmation dialog can ask a question that has nothing to do with git without learning a
second thing.

**`QuillApp::close_tab` writes a modified tab before closing it.** Every other editor puts a
three-answer dialog here; Quill can give the simpler answer because it saves plain text and nothing
else, so writing the buffer to the file it came from is exactly what was typed. A picture tab and a
tab with no path are the two exceptions, each with a reason, and the untitled one *says* it was
closed without saving rather than putting `untitled.md` in somebody's project.

## A file that moves takes the code that names it with it

Drag a row onto a folder and the file moves and every import, `use` line and `mod` declaration in the
project that named it is rewritten. `Rename...` goes through the same function, because a rename
**is** a move to a new name and two answers to one question would be one too many.
`tasks/task-1681-file-operations-tdd.md` is the design.

**The tier is the syntactic one `task-1675` and `task-1680` already chose**, and one more reason is
added to theirs: a refactor that depended on a language server would work on a machine that happened
to have one and *silently do nothing* on a machine that did not — which is the worst of the three
possible outcomes, because the file has already been dropped somewhere else.

**It is one reading, asked twice.** `quill_core::imports` gained the forward half of the questions it
already answered backwards from a caret: `specifiers_in` runs `context_at`'s own test — is this
string inside an import statement — over every string in the file, and `use_statements_in` and
`paths_in` read the path family the same way, out of the token stream so a `use` line quoted in a doc
comment is left as prose. The completion popup and the refactor therefore cannot come to different
conclusions about what an import is.

**One rule decides every case**: *work out what the written text will mean **after** the move; if
that is not what it means now, rewrite it, and if it is, leave it exactly as it is.* That is what
makes moving a whole folder cheap — every specifier inside it still points where it did — and it is
what keeps a `super::sibling` in a moved Rust module untouched while rewriting a `super::sibling` in
a file that stayed behind.

`services::file_move::plan` is all of it, and it reads nothing itself: it is handed a closure that
answers with a file's text, so the window can prefer an open tab's live text and a test can hand in
a map. **The ownership rule is `task-1675`'s, unchanged** — an open file is one
`Command::ReplaceMany`, which is one undo step, and is left **modified rather than written**; a
closed file is read, checked and written once, and one that changed underneath the plan is skipped
whole and named.

**Rust's `mod` declarations are the part that actually breaks the build**, and they are the reason
this is more than a search and replace: a file is not a module because of where it sits, it is a
module because some other file says `mod name;`. So the declaration is taken out of the old parent
module file with any `#[…]` or `///` lines that belong to it, and put into the new one in
alphabetical order with the same visibility — and when the destination folder has no module file at
all, that is a **note**, not a guess. Making `mod.rs` where there was none is a decision about the
shape of somebody's crate.

**There is no preview modal, and that is not the same decision `Rename Symbol` made.** A name is
ambiguous and a path is not: a specifier resolves to one file or to no file, and one that resolves to
no file is left alone, so there is nothing for a person to disambiguate. And **the move is its own
inverse** — drag it back and every specifier comes back exactly as it was, because they were derived
from the paths in the first place. What replaces the preview is a report in the status bar naming
what moved, how many references were rewritten in how many files, and every file that was skipped;
`quill-cli explorer move --dry-run` prints the whole change set and touches nothing, which is the
preview for anybody who wants one and is what a test asserts against.

**The plan is built on the frame the row is dropped**, so it has to be quick: measured against this
repository — 1,001 files in the walk — a Rust module file is 62 ms and a whole module folder, 56
references in 37 files, is 56 ms. It was 557 ms when first written and the fix was removing waste,
which is `task-1666`'s rule again: `syntax::scan` was being run five times over every file, and
`imports::Tokens` is that reading done once; and `nesting` searched the comment list at every byte,
where the list is in order and an index into it is enough. `quill-cli explorer move --dry-run`
prints the elapsed time, so the number stays measured rather than remembered.

**The explorer reports the drag and decides nothing**, which is the shape `task-1673` gave the tab
drag: it collects the rectangle of every row as it draws them and works out which one the pointer is
over once the list is drawn. A folder row is that folder, a file row is the folder the file is in,
and three drops are refused by simply not offering a target — a folder into itself or anything under
it, a path into the folder it is already in, and anything outside the panel, the last so a drag can
be thought better of.

## A terminal tab is called what a person calls it, and sits where they put it

`task-1682` asks for three things a control could only be reached with the pointer for. Right click a
terminal tab and the menu has `Rename...`, `Close` and `New Terminal Tab` on it; drag a tab and it
moves along the strip; and a modal is answered by pressing Enter rather than by finding its button.
`tasks/task-1682-terminal-tabs-tdd.md` is the design.

**A name a person typed beats a name a program set, and nothing a program does takes it away.** It is
a third field on `Session` rather than a value written into `title`, and that is the whole decision:
`claude` sets a title on every prompt, so a rename written into the title would appear to work and
then quietly undo itself the next time the program spoke. It is on the session rather than beside it,
because `quill_terminal::Tabs` is a list of sessions and nothing else, and a parallel list of names
would be a second thing to keep in step with every open, close and move. An **empty** name puts the
tab back to being named after its program, so there is one way to undo a rename rather than a second
command meaning "forget the name I gave"; the dialog cannot ask for it, because its button needs
something in the field, but `quill-cli terminal rename --tab 0` can.

**A name a person typed is never numbered.** `Tabs::names` puts a number after a repeat so that two
tabs running the same program can be told apart, and a person who has called two tabs the same thing
has already said what they want them called. That numbering had a fault this found, and it is the
reason renaming was asked for: it counted names that had already been worked out, and
`powershell.exe 2` does not end with `powershell.exe`, so the **third** shell was a second
`powershell.exe 2`. It counts the names the sessions give, through a map.

**Every entry in the menu is about the terminal tab that is showing**, which is `actions::tab_menu`'s
rule restated, so they are ordinary parameterless actions the View menu, the keyboard and
`quill-cli action run` can all ask for — and a right click therefore **shows** the tab first.
`Rename Terminal Tab...` is on the `View` menu as well, because `quill-cli action list` is built by
walking the real menus and a context menu is not one of them.

**The drag is `task-1673`'s, settled by the strip rather than by the window.** `file_tabs::Strip` and
`file_tabs::insertion_mark` are used unchanged — a tab goes after every tab whose middle the pointer
has passed — but there is **one** strip of terminal tabs, so the strip a tab is picked up from is the
strip it is dropped on and nothing outside `tab_strip` could know better where it landed.
`Tabs::move_tab` is the move, `quill-cli terminal move` calls it too, and `position` counts the tabs
as they are on the screen now including the one being carried, which is what `OpenFiles::drag_tab`
already means by a position.

## Enter answers a modal, and a modal takes the keyboard

**`components::modal::footer` is where that is decided**, so a dialog written later gets it without
asking. Its last button is the one that does the thing and is filled in the accent colour; Enter
presses it. A footer whose last button is dimmed is a modal there is nothing to confirm, so the key
press is left alone rather than doing nothing loudly. The two dialogs that draw their own footer —
`prompt_dialog`, which the text prompt and the confirmation share, and the Settings window — ask the
same function rather than answering a second way.

**The commit panel is the one exception and uses the command key with Enter**, which is IntelliJ's
own chord for the same dialog: its message is a `TextEdit::multiline`, where Enter is a new line and
has to stay one. Both of that modal's tabs use it — one modal, one key — because Enter alone in the
`Stashes` tab would pop a stash for somebody who pressed it meaning nothing. `Go to File`, `Find in
Files` and the references modal need no exception: each takes Enter for itself *before* its footer is
drawn, where it means "open the row that is chosen".

**The modifiers are asked with `is_none` and `command_only`, never compared for equality.**
`consume_key` matches by `Modifiers::matches_logically`, which only asks whether the modifiers the
*pattern* names are held, so a pattern of `NONE` would take `Command+Enter` too — the trap
`task-1678` already recorded. And an equality test against `Modifiers::COMMAND` passes a test and
fails in the window, because **on Windows `Ctrl+Enter` arrives with both `ctrl` and `command` set**.

**`a_modal_has_the_keyboard` is the other half of `text_box_has_the_keyboard`**, and it had to come
with all of the above. That question is `ctx.text_edit_focused()`, so it says nothing about a
confirmation, an about box or most of the git dialogs, none of which has a field in it — and behind
those the editing area, the terminal and the explorer went on reading the frame's keys. With Enter
given a meaning, `Enter` in the delete confirmation would have deleted the file **and** put a new
line in the file behind it **and** opened the row the explorer's cursor was on. It asks egui's own
modal layer rather than a list of Quill's dialogs, so a modal added later is covered without being
added anywhere, and it is the layer as it stood at the **end of the last frame** — which is the
honest answer at the point those three read the keyboard, before anything this frame has drawn.

## A run configuration is a named command, and a run is a terminal with a program in it

`task-1683` asks for IntelliJ's run configurations and `task-1684` is the implementation. A
configuration is a **named command line**, a folder and some environment variables — one kind, not a
template per language, because the surveyed templates all compose into one command line wearing six
boxes. Pressing play spawns a `quill_terminal::Session` with the program in place of the shell, so
the output is a real terminal and stopping is killing a process Quill owns.

**No shell runs the command line.** `run_configurations::split_command` splits it the way a shell
splits a double-quoted word and the parts are handed to the process as arguments, so nothing
expands, nothing globs, and `&&` is one program with a strange argument rather than two programs.
A backslash is a backslash unless it is in front of a quote, because half the paths on Windows have
one in them. Somebody who wants a shell writes `pwsh -Command ...` and has said so where it can be
seen.

**They live in `.quill/run-configurations.conf`**, numbered `run.N.*` the way `files.panes` numbers
a list, written by the released binary only. What is per-person goes in `workspace.conf` beside the
terminal's flags: `run.selected` and `run.visible`. A **temporary** — what running a file or a
suggestion makes — is capped at five and deliberately **never written down**, because a file the
project shares should hold what somebody chose to keep; `Save` in the dialog promotes one. A
remembered `run.selected` is only adopted if something still answers to it, which is usually not
true of a temporary.

**The run tile is the terminal tile's sibling**, not a second tile that resembles it: the same
header, the same padding, the same splitter, and the grid itself is `terminal_panel::grid`, shared
rather than copied. **The bottom of the window holds one of the two** — two grids stacked take the
editing area below the fold — so every path that shows either goes through
`show_the_run_tile` or `show_the_terminal_tile`. That pair exists because leaving it to each caller
did not survive first contact: `quill-cli terminal show` set its own flag, left the run tile up, and
drew both grids into the same rectangle.

**A run records what it ended with rather than re-reading it.** `Session::exit_code` is the source
and `Run::ended` is where the answer is kept the moment it arrives: a program Quill killed has no
code to be asked for afterwards, and a tab that says `exit code 101` has to go on saying it. The
code goes in the tab's **strip**, never into the grid — IntelliJ prints its epilogue into the
console, and a line pretending to be program output is the confusion a separate strip avoids.

**Stopping is soft then hard.** The first press is the interrupt byte down the pty, which the
program can catch; a program still alive two seconds later, or a second press, is killed through
`Session::kill`, which shuts the reader loop down and drops the pseudoterminal — the same path
closing a terminal tab already takes, without dropping the session, so the grid survives the
program. The window is woken **once** when the grace runs out rather than kept drawing for the whole
two seconds.

**A pseudoconsole must be opened at the size it will be drawn at, and never resized while its
program is starting or after it has ended.** Both halves lose the program's output, because
`ResizePseudoConsole` makes the console host re-render its buffer — and that buffer is being written
into at one end of a program's life and empty at the other. It was measured on `cmd /c echo
something`, which writes and exits inside a millisecond and so was **always** still starting when
the tile drew its first frame and told it the real size: its tab came up empty every single time,
and `node hello.js` two runs in three. `QuillApp::run_grid_size` therefore works the size out from
the rectangle the tile really has — recorded on `RunPanel::tile` every frame, whether the tile is
showing or not — so there is no resize at all; `Session::resize` refuses to tell a program that has
ended; and `terminal_panel::grid` **pumps before it resizes**, because whether a program has ended
is only known once the events it sent have been read.
`a_program_that_prints_and_stops_leaves_what_it_printed_in_its_tab` is the guard, and it fails on
the code as it was.

**Plugins contribute data, not types.** The answer to "should running node mean a Node plugin" is
no: node is how JavaScript runs, and the JavaScript manifest says so itself with `run.file = node
{file}`. `run.project` names a detector **built into Quill**, checked against
`plugins::PROJECT_RUNNERS` exactly as `language.renders` is checked against `RENDERERS`; `cargo`
reads `Cargo.toml` and `npm` reads a `package.json`'s scripts, both in Quill's own code, so the most
a third-party manifest can do is suggest text, visibly. Rust names a detector and **no** file
runner, because running one file of a Cargo project is not a thing cargo does — so `Run Current
File` is absent for a `.rs` file rather than offered and wrong.

`quill-cli run` is the whole feature from the command line, and `run output` is the one to notice:
it reads the run's `Screen`, so an agent can start a dev server, read its port out of the log,
exercise it and stop it with nobody watching.

## A terminal opens in the project, running the shell the person actually uses

`task-1670` reported a terminal that opened in `C:\Windows` and could not find the machine's own
commands. Four rules came out of it, and a change to the terminal or to what Quill remembers has to
keep all four.

**A path Quill writes down or hands to a program is plain.** `std::fs::canonicalize` on Windows gives
back a **verbatim** path — `\\?\C:\jason\dev\quill` — and every Rust file call takes one happily, so
nothing inside Quill notices while it travels: into `recent.txt`, onto the explorer's root, and from
there to the directory a shell is started in. `cmd.exe` is where it stops, because two leading
backslashes are a network share as far as it is concerned; it says so and starts in `C:\Windows`
instead, which is a terminal that opens, works and is quietly in the wrong folder.
`quill_terminal::paths::plain` takes the prefix off. `Store::remember_project` calls it so one is never
written down, `Store::recent_projects` calls it while reading so a list already on disk is repaired,
and `Session::spawn` calls it again at the point the shell is started — because the window is not the
only thing that hands a directory over, and a list of the places that have to remember to strip it is
a list whose next entry will be the one that forgot.

**`COMSPEC` is not the shell.** It names the interpreter that runs a batch file and says `cmd.exe` on
every Windows there is, so reading it meant Quill's terminal never held the commands in a person's
PowerShell profile. The default is `pwsh.exe` when it is installed and `powershell.exe` otherwise —
they read **different** profiles, so it is not a preference between two spellings of one shell — and
`terminal.shell` in the settings is how a person asks for something else back. Empty means "what this
machine says", and `Settings::shell()` is the one function that says so.

**With no path on the command line, where Quill starts depends on how it was started.** The current
directory is the honest answer when somebody typed `quill` in a terminal and no answer at all from a
desktop shortcut, where it is only wherever the shortcut points. `quill_app::starting_folder` reopens
the last project **only** when the current directory is the folder `quill.exe` itself lives in. Narrow
on purpose: `quill` typed in a folder has to open that folder.

**What a menu calls a key is not what a terminal sends.** `actions::key_name` spells the punctuation as
words — `Backslash`, `OpenBracket` — because `Ctrl+Backslash` reads better in a menu. The key encoder
used to ask it, and a word is not one character, so `Ctrl+]`, `Ctrl+\` and `Ctrl+Space` were sent as
nothing at all — and `Ctrl+]` is how a person detaches from `claude`.
`components::terminal_panel::symbol` answers the terminal's question instead. It also refuses a
**shifted** digit or symbol, because `Shift+4` is `$` here and `"` on a British layout and there is no
control code to be had from a key whose character depends on the keyboard; a letter is untouched,
since `Ctrl+Shift+C` is `Ctrl+C` in every terminal there is.

**And what egui calls a copy is not always one.** `task-1671` reported that `Ctrl+C` could not stop a
command. The encoding was never wrong — `keys::encode` has turned it into `0x03` since the terminal
was written — but the key press never arrived: `egui-winit` asks whether a press is a clipboard
command *before* it pushes a key event, and `is_copy_command` is `modifiers.command && key == C`. On
macOS `command` is the Apple key, so `Ctrl+C` is an ordinary key press and always worked; **on
Windows `command` is the control key**, so every `Ctrl+C` became an `Event::Copy` with no key event
and no text event behind it. `terminal_panel::clipboard_key` is where the choice is made now, the way
every terminal on Windows makes it: something selected and `Ctrl+C` copies it **and lets go of the
selection** — left behind it would swallow the next press too — nothing selected and it interrupts,
`Ctrl+Shift+C` always copies, and `Ctrl+X` reaches the program as `0x18`, because nothing in a
terminal can be cut and that is how a person leaves `nano`.

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

## A zoom keeps the line you were reading where it was

`task-1672` reported that zooming in meant scrolling back to the line you had zoomed in on. The
editing area is not a `ScrollArea`: a tab holds `OpenFile::scroll`, a number of points from the top
of the document, and that number means something different the moment the text is laid out at a
different size. `app/mod.rs` is 5,900 points tall at nine points and 10,500 at sixteen, so a scroll
position that did not change was a reader half a file away from what they were looking at.

**What is remembered is the text, not the offset.** `quill_core::Anchor` is where the line the point
fell on starts and how far down that line it sat; `Layout::anchor_at_y` takes one before the size
changes and `Layout::y_of_anchor` puts it back after, so the line is worked out again rather than
remembered and a paragraph that wraps differently at the new size still has an answer. The fraction
is what makes it exact rather than nearly exact — anchoring on `caret_at`, which returns the glyph
box, loses where in the line the point was and drifts a line or two over a whole gesture.
`Layout::line_of_anchor` is the off-by-one that had to be got right: `line_of_offset` gives the
**earlier** line for an offset on a line break, which is right for a caret ending a line and wrong
for an anchor, because a wrapped line starts at the byte the line above ends at.

**The anchor lives on the tab**, beside the scroll position it corrects, as `OpenFile::zoom_anchor`
and `preview_anchor` — one for each thing that scrolls. It is taken before the size changes and used
on the first frame the file is laid out again, which for a tab nobody is looking at may be much
later and is still the right answer, because a file that has not been laid out again has not moved.
`set_the_font_everywhere` takes one for **every** open file, so a tab comes back at the line it was
left at; an anchor already taken is left alone, so the pointer's anchor — set first by the pane
being zoomed — beats the top-of-view one. It is cleared exactly where a document is **replaced**,
and must not be cleared by `forget_what_was_worked_out`, which is also how showing a tab throws
away what was laid out for it.

**Which point stays still depends on how the size was changed**: the pointer for a pinch or
`Ctrl`+wheel, the caret clamped into the view for the keyboard, the top of the view for the Settings
window and for every tab that has neither. The pointer is asked for as
`hover_pos().or(latest_pos())` because egui reports no pointer at all on a frame whose only input is
a wheel event.

**A gesture belongs to the window, not to a pane.** `zoom_the_text` was called from `show_editor`,
once per pane, and each pane accumulated the same `zoom_delta`: with the editing area split, one
notch of the wheel took sixteen points to thirty two. It is claimed once a frame now and the pointer
decides whose it is — a pane under the pointer takes it, and the pane with the keyboard takes it at
the end of the frame if no pane turned out to have the pointer. Settled after the pane loop, because
a pane earlier in the row must not claim a gesture aimed at one later in it.

The correction lands on the frame **after** the size changed, since `set_font_size` only marks the
layout stale. An idle window draws nothing, so `set_the_font_everywhere` asks for a repaint —
without it the last notch of a gesture sits uncorrected until something else wakes the window.

## The source and its preview scroll together, through the text rather than the height

`task-1673` asks that scrolling either half of the side by side view scroll the other. The obvious
answer — the same fraction of each half's scrollable height — is wrong, and gets worse the further
down a document you go: a heading is one line of source and half again as tall on the page, a fence's
backticks are two lines of source and nothing at all, and a picture is one line of source and four
hundred points of page. Measured on a plain sixty section document with none of that in it, the two
pages already differ by thirteen per cent.

**What both halves agree about is the text**, so `quill_core::markdown::Preview` carries a fourth
structure beside its text, spans and paragraph styles: `source_lines`, which line of the source each
line of the preview came from. `render` sets `builder.source_line` once at the top of its walk and
`end_line` records it, so none of the nine branches has to remember. It never goes backwards, which
makes finding a line a binary search, and it is exactly as long as the preview has lines — the test
that already checked the other three structures agree checks the fourth too, because one that drifted
would be a scroll to a paragraph that is not there.

`quill_core::scroll_sync` is the crossing, two pure functions and no state: which **paragraph** is at
this height and how far down it the point sits, which line of the other page that is, and where that
line ended up at the same fraction. A paragraph rather than a line, because one source line wraps to
five lines at one width and two at another while its paragraph number means the same in both — which
is what `Layout::paragraph_band` and `paragraph_at_y` are for. The fraction is what makes it smooth
rather than stepped a paragraph at a time.

**Which half drives is decided by which one moved**, compared against where both were before the
frame drew anything. That is not decoration: the crossing snaps to a paragraph, so a position taken
across and back is not quite where it started, and a rule that wrote to both halves every frame would
creep down the file on its own for as long as the window was left open. Both moving means a change of
font size, which `task-1672`'s anchors have already corrected, so that does nothing either. The
follower lands on the next frame, sixteen milliseconds later, which nobody can see.

`follow_the_other_half` is split out because **the command line needed it**: `quill-cli editor scroll`
is applied at the top of a frame, before anything is drawn, so the frame's own before-and-after
comparison would see nothing move.

## A tab is dragged, and where it lands is the window's decision

Rearranging tabs in one strip would be the strip's own business. Dragging one into **another pane** is
not, because each pane draws its strip inside a `Ui` of its own and has never heard of the others,
while the pointer wanders freely between them.

So the strip does what every component in Quill does — it reports what happened and decides nothing.
It says that a tab is being carried and where the pointer is, and it reports `file_tabs::Strip`: where
it drew itself and each of its tabs. The window collects one a pane in the loop it already runs, and
`settle_the_tab_drag` runs once afterwards, which is the earliest moment anything knows where every
strip is.

Three rules it settles by. **A tab may be dropped anywhere in a pane**, not on its strip alone, which
is what IntelliJ does and what a person dragging a file into the pane beside them is aiming at. **It
goes after every tab whose middle the pointer has passed**, which is what makes it follow the pointer
rather than jump when it crosses an edge. And **dropped outside every pane nothing happens**, so a
drag can be thought better of.

`OpenFiles::drag_tab` does the move and is what `quill-cli tab move` calls too. Its one subtlety:
`position` counts the target pane's tabs **as they are on the screen now**, including the tab being
carried when it is already in that pane, so a move further along its own strip has one subtracted
from it there rather than at every call.

## A document has a thin bar down its right hand edge

`components::scrollbar`, on the editing area and on the Markdown preview. Three things about it are
worth knowing before touching it.

**Where it can go is decided by what else wants that point.** Six points in from the right of its
pane: exactly what `components::resize_edges` takes from the window's edge and what the activity bar's
buttons are inset by, which leaves two points clear of a pane divider's grab area. It lands inside
`EDITOR_PADDING_X`, so no letter is ever drawn under it.

**It is two calls rather than one**, which is the only place a component in Quill is. `grab` takes the
pointer where the pane takes its own, because egui hands a drag to the last widget that asked for the
point and the editing area asks for all of it — a bar interacted with first cannot be dragged. `paint`
draws at the end, once the wheel and the caret have had their say, because a thumb drawn from the
scroll position the frame opened with is a frame behind the writing.

**The fade is between two palette colours, not down one colour's alpha.** At the alpha that reads as
subtle against `EDITOR`, the idle thumb was nine values a channel from the background and was not
honestly a scrollbar. Quiet it is a five point mark in `CONTROL`; used, eight points in `TEXT_DIM`
with the track behind it. It is never taken away altogether, because a bar that disappears stops
answering "how far through this am I".

One thing it forced: the wheel is gated on `contains_pointer()` rather than `hovered()`. A widget over
the editing area takes its hover, so with a bar there a wheel turned with the pointer on the bar
scrolled nothing. `contains_pointer` asks whether another **layer** is covering the rectangle, so a
bar in the same layer no longer takes the wheel away and a popup over the editing area still does.

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

## An agent is given the catalogue, not a second list

Quill has a Model Context Protocol server, so a client that speaks it — Claude Code, Codex, anything
else — is handed the commands as tools rather than being handed `docs/commands.md` and told to shell
out. `quill_cli::mcp` is all of it, in the CLI crate rather than the window, because that is the way
the dependency already points: the window can host it without the client ever learning what a window
is. `task-1679` is the design, `quill-cli/docs/mcp.md` is what a person needs.

**The tools are generated from `catalogue.rs`.** This is the fourth rule in the section above and it
is the reason the module exists: a command added to Quill is a tool the day it is added, with its
summary, its arguments and its flags, and `every_command_is_offered_as_a_tool_in_both_shapes` fails if
one ever is not. Do not write a tool out by hand. If a tool needs to say something the catalogue does
not, the catalogue is what should say it — the summary you are writing has a third reader now, and it
is the one least able to ask what was meant.

**One command is held back, and the list of exclusions is a test.** `mcp serve` would start a server
from inside one. `tools::offered` is where that is written down and
`exactly_one_command_is_held_back...` fails if the list grows, because an exclusion nobody argued
about is how "everything is reachable" quietly stops being true.

**One tool an area is the default, and it was measured rather than assumed.** A hundred and four
commands are a hundred and four tool definitions in an agent's context on every conversation — about
**18,800 tokens**, against **8,200** for the fifteen area tools, which still carry every command's usage line
and summary. `quill-cli mcp tools --count` prints both figures against the catalogue as it is now, so
the choice is never made against a number in a comment. `mcp.tools = every` is there for a client that
permits tools by name, which is the one thing grouping really costs.

**The server holds no session, and that is deliberate.** MCP `2025-06-18` has an `initialize`
handshake and an optional session id; `2026-07-28` deleted both. A server that never *requires*
`initialize`, issues no session id and echoes back whatever version the client named answers both with
one code path. Do not add a version switch, and do not add a session.

**A tool call goes down the control channel.** It becomes exactly the request `quill-cli` would have
sent — same wire name, same arguments, same token, same port — so `run_cli` stays the one place a
command becomes a change. Two things follow, and both are wanted: one server drives every open window,
so two Quills sharing one `mcp.port` is the behaviour rather than a collision; and a window started
with `--control off` has nothing for a server to drive, which the page says rather than listening
uselessly.

**The HTTP endpoint is off by default and should stay that way.** The stdio server an agent launches
needs no port and lives as long as the conversation; a fixed open port will run `terminal send` for
anything that can reach it. The browser case — the one thing a loopback port really has to defend
against — is closed by refusing a non-loopback `Origin` and a cross-site `Sec-Fetch-Site`, which is
what the specification asks for and what the token does for the older channel.

**The installers use the agent's own command first.** `~/.claude.json` is rewritten by every running
Claude Code and is a hundred kilobytes of somebody's settings; Codex's `config.toml` is hand-written
and holds comments. `claude mcp add-json` and `codex mcp add` do the locking. The direct edit is the
fallback, it takes a copy first, and it changes one key or one table and nothing else.

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

**And it is read in one place too**, `build_info`, which also holds the build date. The date is
stamped by `crates/quill-app/build.rs` while the binary is compiled — the local time of the machine
that built it — so it is never edited by hand and cannot be forgotten. That file's comment says when
the stamp moves and why it does not move more often than that: the value is part of the crate's
fingerprint, so a script that restamped on every invocation would recompile `quill-app` and relink
every screenshot test each time the clock ticked. `components::about_dialog` is what shows the two,
and it takes them as text rather than reading them, because a picture that changes every build cannot
be a screenshot test.

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

**CSS is what made the tokeniser take instructions.** `task-1671` asks for a plugin like the other
three, and writing the manifest was half an hour's work that would have produced a bad plugin. CSS
breaks three of the rules in `quill_core::syntax`: **a hyphen is a letter** — nearly every property
name has one in it, every custom property starts with two, and a pass that split a word there could
not name a single property; `#ff0000` is a **colour**, and `number` wants a digit first, so half the
colours in a file were coloured and half were not; and a stylesheet has **three** kinds of word worth
telling apart — the at-rule, the property and the value — where a grammar had two lists.

So `Grammar` gained three fields, each a manifest key, each **off unless a language asks for it**, so
no plugin that shipped before changes by a pixel: `language.word_characters` (characters that are
part of a word wherever they appear, `-` and `@` here), `language.types` (a third list, producing
`Token::Type`, which until then was reachable only by the capital letter heuristic), and
`language.hex_colors`. `plugins::tests::the_older_plugins_ask_for_none_of_what_css_added` is what
keeps them opt-in.

Which word goes in which list is the whole of the plugin's design, and one rule decides the awkward
cases: **a word that is both a property and a value is coloured whichever way it is written more
often**. `inset`, `left` and `content` are properties; `flex`, `grid` and `all` are values, because
`display: flex` and `transition: all` are far commoner than the shorthand properties of the same
name. `tasks/task-1671-css-plugin-tdd.md` has the table and the rest.

A bundled plugin's icon is generated rather than drawn, and each one records how:
`plugins/mermaid/icon.md` and `plugins/css/icon.md` each have the prompt, the endpoint and the two
commands, so it can be made again without guessing.

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
- `tasks/task-1673-split-view-tdd.md` — the source and its preview scrolling together: why a
  proportion of the height is the wrong crossing and what the fourth structure in a `Preview` is,
  dragging a tab within a strip and into another pane, and the scrollbar — where it can go, why it is
  two calls, and what "more subtle" was made to mean.
- `tasks/task-1663-highlights-tdd.md` — highlighting a passage: where the ranges live so they move
  with the text, the file beside the project that remembers them, the right click menu and the drawn
  colour wheel, and the bulk commands.
- `tasks/task-1670-terminal-tdd.md` — the terminal that opened in `C:\Windows` running the wrong
  shell: the verbatim Windows path and where it came from, why `COMSPEC` is not the shell, when Quill
  reopens the last project, and the punctuation keys that reached no program at all.
- `tasks/task-1671-css-plugin-tdd.md` — the CSS plugin: the four shapes of CSS the tokeniser could
  not read, the three grammar keys added for them, which CSS word goes in which of the three lists,
  and why `Ctrl+C` reached no program on Windows.
- `tasks/task-1672-zoom-tdd.md` — the zoom that kept the line you were reading: why a scroll
  position cannot survive a change of size, the three ways of putting the view back that were
  weighed, where the anchor lives and when it is cleared, and the split view where one notch of the
  wheel was worth two sizes.
- `tasks/task-1666-performance-tdd.md` — why a frame cost 818 ms and now costs 20: the eight faults
  that were found, what each was worth, the two revisions a document counts, the incremental layout
  and why its fingerprint is derived rather than reported, and what was deliberately not done.
- `tasks/task-1677-autocomplete-tdd.md` — auto-complete: what each tier of editor does and which of
  their behaviours is worth copying, why the candidates are the ones `task-1676` already keeps rather
  than a word database of their own, the Sublime scoring rubric restated for identifiers, the five
  keys and why `Tab` and `Enter` differ, and the thirty-five scenario battery. `task-1678` is the
  implementation of it; `cargo run --release -p quill-app --example completion_cost` is how its one
  budget is measured again.
- `tasks/task-1680-import-completion-tdd.md` — completing an import: the two shapes a module is
  written in and the one enum that reads both, why the tier is the project's file list and a
  syntactic reading rather than a language server, the nine manifest keys and what a list of
  languages inside Quill would have cost instead, what auto-import would take, and the fifty-one
  scenario battery.
- `tasks/task-1682-terminal-tabs-tdd.md` — renaming a terminal tab, dragging one along the strip,
  and answering a modal with Enter: why a name a person typed is a third field rather than the title,
  the numbering fault that made the third shell a second `powershell.exe 2`, why the strip settles
  its own drag where a file tab's is settled by the window, the one modal whose body owns Enter, and
  why a modal had to start taking the keyboard from the panes behind it.
- `tasks/task-1681-file-operations-tdd.md` — deleting a file, saving a tab that is closed, and
  moving one with its references: what the surveyed editors do about each, why the explorer had to
  be able to hold the keyboard and what hands it back, why a deleted file goes to the Recycle Bin on
  one platform and not the other, the two families the move refactor rewrites and the `mod`
  declarations that are the hard half, and why this one has no preview modal when rename does.
- `tasks/task-1675-code-editing-tdd.md` — go to definition, find all references and rename: the
  three mechanisms that were weighed (a language server client, tree-sitter and stack graphs, a
  syntactic index) and why the index was chosen, the two grammar keys a language adds, the
  references and rename modals, and the fifty-scenario battery the implementation is held to.
  `task-1676` is the implementation of it; `cargo run --release -p quill-app --example symbol_cost`
  is how its budgets are measured again.
- `tasks/task-1685-markdown-tdd.md` — the Markdown preview: the twenty-eight things the
  line-at-a-time parser could not express and why they were all one fault, why a Markdown crate was
  weighed and refused, the block tree and the delimiter stack that replaced it, the three ways of
  giving a glyph-placing layout engine a column and why the table is set in the code font, what
  selecting in a read-only page needs, and what is deliberately left out.
- `tasks/task-1683-run-configurations-tdd.md` — run configurations: what IntelliJ's model, widget
  and Run tool window each are, why a configuration in Quill is one named command rather than a
  template per language, the run tile built on the terminal stack, the two manifest keys and the
  built-in detectors that answer "should node be a plugin", and what a debugger would need that
  this deliberately leaves visible. `task-1684` is the implementation of it, and the section above
  on running is what it left behind.
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
- `tasks/task-1667-version-and-release-tdd.md` — the About box, the build date stamped at compile
  time, and the one-command release: why the date is not a hand-written number, not the executable's
  mtime and not a dates crate, and why the instruction is a script rather than a paragraph.
- `installer/README.md` — how to build an installer, on either platform.
- `tools/release.ps1` — the one command that releases: bump, build, install, tag, push, publish.
- `tasks/improvements.md` — the ask that the settings window, the panes, the terminal and the menus came
  from.

Each document stands on its own. If a fact from another one is needed, state the fact.
