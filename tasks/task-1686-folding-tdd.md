# task-1686 — Collapsing and expanding blocks

## 1. What was asked for

> On the left of functions, if statements, large code blocks, we want a up/down carret, to the right
> of the line number, that lets us expand/collapse that section, just like in IntelliJ.
>
> Do online research, and create a tdd.
>
> We want a new right click menu option to collapse all but highlighted, show all again, so that we
> can focus on the highlighted sections of the file. Keyboard shortcut, cli, and mcp tools should
> exist for this as well.
>
> the line numbers should remain correct for the highlighted items, collapsed items, etc.

Six things, and the sixth is the one that decides the design:

1. An arrow in the gutter, to the right of the line number, against a foldable block.
2. Pressing it collapses that block; pressing it again brings it back.
3. Functions, `if` statements and large blocks are foldable — so what is foldable has to be worked
   out from the file rather than typed in.
4. `Collapse All But Highlighted` and `Show All Again` on the editing area's right click menu, so a
   person can read only the passages they have marked.
5. Keyboard shortcuts, `unluminous-cli` commands and MCP tools for all of it.
6. **The line numbers stay correct.** Line 400 is line 400 whether the four hundred lines above it
   are showing or not.

Point 6 rules out the cheapest implementation, which is to build a second document holding only the
visible lines and lay that out. Every offset in Unluminous — the caret, the selection, the marked
passages, the syntax spans, the search hits, the definitions index — is a byte offset into the real
file. A shadow document would need every one of them translated in both directions at every seam,
and the first seam somebody forgot would put a highlight on the wrong word. What is wanted is the
real document, laid out with some of its paragraphs left out.

## 2. What other editors do

### 2.1 IntelliJ IDEA

The reference for the ticket. Its arrows are in a *fold gutter* between the line numbers and the
text, drawn against the first line of every foldable region. A collapsed region shows the head line
with the body replaced by a `{...}` placeholder that can be clicked to expand. Regions nest, and the
keyboard reaches them at three depths:

| What | Windows / Linux |
|---|---|
| Fold or unfold the block at the caret | `Ctrl+NumPad-` / `Ctrl+NumPad+` |
| Collapse or expand everything | `Ctrl+Shift+NumPad-` / `Ctrl+Shift+NumPad+` |
| Recursively, this block and everything in it | `Ctrl+Alt+NumPad-` / `Ctrl+Alt+NumPad+` |
| Expand to a nesting level | `Ctrl+NumPad*`, then 1–5 |
| Fold the selection as a custom region | `Ctrl+.` |

What is worth copying: the arrow lives beside the line number rather than in the text; the head line
stays; a collapsed region has a visible placeholder that is itself the way to expand it. What is not
worth copying: the numeric keypad. Half the keyboards in this house have no numeric keypad, and
`Ctrl+NumPad-` on a laptop is not a shortcut, it is a riddle. §8 says what Unluminous binds instead.

### 2.2 Visual Studio Code

Two mechanisms, chosen by `editor.foldingStrategy`. A **folding range provider** — the language
server, or the built-in TypeScript service — is used when there is one; otherwise the **indentation**
model, which the language configuration guide states in one sentence:

> A folding region starts when a line has a smaller indent than one or more following lines, and ends
> when there is a line with the same or smaller indent.

Empty lines are ignored. A language may also contribute `folding.markers`, a pair of regular
expressions — `//#region` and `//#endregion` — that make an explicit region.

Its commands are the ones the ticket's fourth point names: `editor.foldAll` (`Ctrl+K Ctrl+0`),
`editor.unfoldAll` (`Ctrl+K Ctrl+J`), and **`editor.foldAllExcept`** — "Fold All Regions Except
Selected", `Ctrl+K Ctrl+-`, which folds everything and then unfolds the regions the selections are
in, along with their parents. That command is exactly the ticket's `collapse all but highlighted`,
with a selection where Unluminous has marked passages.

### 2.3 CodeMirror 6

Folding is a `Decoration.replace` over a range, held in a state field, and the folded set is queried
with `foldedRanges`. The decorations ride the document's own position mapping, so an edit above a
fold moves the fold rather than breaking it. That is the important idea and Unluminous takes it: **the
fold state is anchored in the document's coordinates and is moved by the same code that moves
everything else when the text changes** — which in Unluminous is the two functions `Document::insert` and
`Document::remove_range`, and nothing else.

### 2.4 The Language Server Protocol

`textDocument/foldingRange` answers with a list of `FoldingRange`: `startLine`, `endLine`, optional
character offsets, an optional `collapsedText`, and a `kind` which is one of `comment`, `imports` or
`region`. Two things are worth taking from it. First, the range is **lines**, not bytes — every
editor that folds, folds whole lines. Second, the `kind` exists so that "fold all comments" can be a
command; Unluminous's [`Kind`](#4-what-is-foldable) is the same idea and the same three or four values.

### 2.5 Sublime Text and Vim

Sublime folds from its syntax definition's scopes and shows `⋯` in the text. Vim's `foldmethod` is
the same catalogue Unluminous is choosing from: `manual`, `indent`, `expr`, `marker`, `syntax`. Vim's
`zc`/`zo`/`zR`/`zM` are the same four commands under different names.

## 3. Which tier decides what is foldable

Three mechanisms, the same three `task-1675` weighed for go-to-definition, and the answer is the
same for the same reasons.

**A language server** would give the truest answer: `textDocument/foldingRange` from the real
compiler. It would also mean a separate program per language, found on `PATH`, holding gigabytes,
answering at a time that depends on the machine — and a fold gutter that appeared a second and a half
after a file opened, or never, depending on what somebody happened to have installed. `task-1675` §2
rejected it and every reason holds here. There is one more: a *refactor* that silently does nothing
without a language server is a bad day; a *fold arrow* that silently does not appear is a feature
that looks broken.

**A parser — tree-sitter, or a hand-written one per language** — would be correct and is code where
Unluminous's plugins are data. Twenty-one languages would be twenty-one grammars to carry.

**A syntactic reading**, from the token stream `unluminous_core::syntax::scan` already produces. That is
what Unluminous does everywhere else — the definitions index, find all references, the import refactor —
and it is what VS Code falls back on for every language with no provider, which in practice is most
of them. It is instant, deterministic, testable with no window, and it costs nothing that is not
already being paid: the file is scanned to colour it, and this is one more reader of the same pass.

So: **a syntactic reading, from the same one pass.**

## 4. What is foldable

`unluminous_core::folding` answers it. One entry point, and the file's kind chooses how it reads:

```rust
pub enum Reading<'a> {
    /// A switched-on plugin claims this file, so its brackets and its comments can be read.
    Code(&'a Grammar),
    /// A Markdown document: its headings.
    Markdown,
    /// Nothing is known about it: its indentation, and nothing else.
    Plain,
}

pub fn regions(text: &str, reading: Reading<'_>) -> Vec<Region>;
```

A `Region` is **whole lines**, as it is in every editor that folds:

```rust
pub struct Region {
    /// The line that stays visible, counting from 0. The arrow is drawn against it.
    pub head: usize,
    /// The lines that disappear when it is collapsed. Never empty.
    pub body: Range<usize>,
    pub kind: Kind,
}

pub enum Kind { Block, Comment, Indent, Heading }
```

`head` is a paragraph index, which is what `unluminous_core` calls a source line everywhere else, and it
is one less than the number the gutter draws.

### 4.1 Blocks — `Kind::Block`

Every `{`, `[` and `(` that closes on a later line than it opened on. The brackets are matched with
a single walk that is handed the comment and string spans from `syntax::scan`, so a brace inside a
`// }` or inside `"}"` is not a brace — which is the same reading `imports::Tokens` already does for
the import refactor, and it is why that walk is shared rather than written twice.

Parentheses are included because a wrapped argument list is a real thing to fold in Rust and
TypeScript, and because leaving them out would mean a rule that says "these two brackets and not
that one", which is a rule with no reason behind it.

**Which lines disappear.** The head is the line the bracket opened on and it stays. The body runs
from the line after it down to the line the bracket closed on — *including* that line, so folding a
function leaves one line on the screen rather than two. There is one exception, and it is the one
that stops the feature being annoying:

> If there is a word character on the closing line after the closing bracket, that line stays.

`});` and `}` and `},` fold away. `} else {` and `} catch (error) {` and `} while (going);` stay,
because hiding the `else` of an `if` somebody folded is hiding the half of the statement they were
trying to see the shape of.

### 4.2 Comments — `Kind::Comment`

Two shapes, both worth folding and both cheap:

- A **block comment** that spans lines — `/** … */`. It comes straight out of `syntax::scan` as a
  single `Token::Comment` covering more than one line.
- A **run of line comments**: two or more consecutive lines whose first non-blank content is the
  language's line comment. The licence header at the top of a file is the case that matters.

The head is the first line of the comment and the body is the rest of it.

### 4.3 Indentation — `Kind::Indent`

VS Code's sentence, implemented literally: a region starts at a line with a smaller indent than the
line or lines that follow, and ends at the last line before the indent returns to that level or
below. Blank lines are ignored, and **trailing blank lines are not part of the region**, or every
fold in a file with blank lines between its functions would swallow the gap after it.

Indentation is what makes the feature work for Python, YAML, and every language nobody has written a
plugin for. It is computed for every file, and then:

> An indentation region whose head line already has a block or comment region is dropped.

In a braces language every block has both, and the block's answer is the better one — it knows about
the closing bracket. What is left over is the indentation of a language with no braces, and the
`match` arms and struct literals of one that has them but did not close on a later line.

### 4.4 Headings — `Kind::Heading`

For a Markdown file, an ATX heading folds everything down to the next heading at the same level or
higher, which is what a person means by collapsing a section. `#` at the top of the file therefore
folds the whole document below it and `######` folds three lines. A heading inside a fenced code
block is not a heading, which the reader already has to know because `#` is a comment in half the
languages people put in fences.

Markdown also gets indentation regions, so a long nested list folds.

### 4.5 What the answer is like

The regions come back **sorted by head, then by widest body first**, and nested regions are allowed
and expected — a method inside a class inside a module is three regions with three arrows.

Two are dropped:

- A region with an empty body. Folding it would hide nothing and draw an arrow that does nothing.
- A duplicate: two regions with the same head keep the wider one. `fn f() {` where the `(` and the
  `{` both open on the same line and close on the same line is one arrow, not two.

## 5. Where the fold state lives

The regions are derived from the text and are not state. What *is* state is which of them somebody
has collapsed, and there are two places it could go.

**On the tab**, as `OpenFile` state, which is where a scroll position and a view mode live. It would
have to be a set of paragraph numbers, and a paragraph number means nothing after an edit: typing
one line at the top of a file would move every fold in it down one, and every fold below the edit
would be pointing at the wrong block until the file was closed.

**In the `Document`**, as byte offsets, which is where the marked passages live and for exactly the
same reason. `Document::insert` and `Document::remove_range` are the only two places in Unluminous that
know a range of bytes moved, and they already shift `chars` and `highlights` in the same two lines.
A third line shifts the folds, and an edit anywhere in the file leaves every fold where it belongs
without anything else in Unluminous having to think about it.

So: **`unluminous_core::folding::Folds` lives in the `Document`**, and it holds the byte offset of the
**start of the head line** of every collapsed region.

Three rules come with it, and each is a rule something else in Unluminous already follows:

- **It rides the undo `Snapshot`**, because undo in Unluminous restores a state and this is part of the
  state the document was in. Undoing back past the moment a block was folded unfolds it. That is
  the same sentence `Highlights` carries, and the alternative — folds that survive an undo — would
  need the snapshot to know how to *not* restore something, which nothing here does.
- **Folding is not an edit.** Nothing goes on the undo history, and the file is not marked as having
  unsaved changes, because what Unluminous saves is plain text and a fold is not in it. The editor's font
  and the marked passages already work this way.
- **A stored offset is snapped to its line.** A fold is collapsed when one of the stored offsets
  falls anywhere inside its head line, rather than exactly on its first byte, so typing at the start
  of the head line does not pop the fold open. Every command that changes the folds writes the set
  back rebuilt from the regions as they are now, so an offset that no longer names any head is
  dropped at the first opportunity rather than kept for ever.

### 5.1 The third revision counter

`Document` counts two revisions today: `revision`, which counts every change of any kind, and
`text_revision`, which counts only what changed the text or its formatting. `task-1666` explains
why: the layout, the syntax colouring and the Markdown preview are all keyed on the second, so
moving the caret does not re-tokenise the file.

Folding changes the layout and changes nothing else. Keyed on `text_revision` it would re-colour the
file and rebuild the preview for a fold; keyed on `revision` the layout would be rebuilt on every
frame of a drag. So there is a **third counter**, `fold_revision`, and `refresh_layout` is keyed on
the pair. It is the same reasoning that produced the second one, applied once more:

| Counter | Bumped by | Read by |
|---|---|---|
| `revision` | anything at all | does the window need painting |
| `text_revision` | the text or its formatting | the layout, the colouring, the preview, the symbols |
| `fold_revision` | collapsing or expanding | the layout, and nothing else |

The invariant `a_layout_that_changed_means_the_text_revision_moved` is unharmed: folding is not a
`Command`, exactly as marking a passage is not.

## 6. How a folded region is hidden

`Layout` is a list of `PlacedLine`s and a `starts` array saying which of them belong to each
paragraph. **A hidden paragraph produces no lines**, and its two `starts` entries are therefore
equal. Nothing else changes:

- The gutter walks `layout.lines` and draws `line.paragraph + 1` against the first row of each
  paragraph. A paragraph that produced no rows draws no number, and every other number is what it
  always was. **That is the whole of the ticket's sixth point, settled by construction rather than
  by arithmetic.**
- The painter draws the lines it can see, which is a pair of binary searches over the `y` positions.
  A file with four hundred lines hidden has four hundred fewer lines in the list and the searches do
  not know the difference.
- `line_of_offset`, `offset_at`, `caret_at`, `selection_rects_in` and `visible_lines` are all walks
  or searches over the same list and all continue to work. An offset inside a hidden paragraph
  resolves to the nearest line that exists, which is where a caret in a folded block is drawn — and
  §9 says why the caret is never allowed to be there.

`layout` and `relayout` gain one parameter:

```rust
pub struct Hidden(Vec<Range<usize>>);   // paragraph ranges, sorted, never overlapping
```

with `Hidden::none()` for every caller that does not fold — the Markdown preview, the diagram view,
the tests in `unluminous-core`, `examples/frame_cost.rs`.

Two things inside `layout.rs` have to change with it, and both are the sort of thing that is a
silent wrong answer rather than a crash if it is missed:

- **The fingerprint carries the hidden flag.** `relayout` decides what to lay out again by comparing
  each paragraph's fingerprint against the last layout's; a paragraph that has just been hidden must
  fingerprint differently or it would be kept, complete with its lines. The module comment on
  `fingerprint` already says that everything `lay_out_paragraph` reads belongs in it, and this is now
  one of those things.
- **`relayout`'s byte shift is taken from the first paragraph in the suffix that produced a line**,
  not from `after[0]`. Those were the same thing while every paragraph produced at least one line.
  They are not the same thing when the suffix begins with hidden paragraphs, and the difference is
  every byte range in the second half of the file being wrong.

`Layout::paragraph_band` returns `None` for a paragraph with no lines, rather than reading the line
before it and the line after it and reporting a band between them.

### 6.1 Two ways that were not taken

**Filtering the lines after laying the whole document out** would be simpler to write and would lay
out four hundred lines nobody is going to look at, on every re-layout, for as long as the fold is
closed — and `task-1666` is a document about not doing that.

**A `min_height` of zero on the hidden paragraphs**, reusing the mechanism the Markdown preview's
pictures already use, does not work: `min_height` is a floor and the line is still as tall as its
letters. Making it a ceiling as well would change what it means for the one thing that uses it.

## 7. The arrow, and the collapsed block

### 7.1 The arrow

`components/gutter.rs` has said this since it was written:

> The 12 point gap to the right of the numbers is deliberate empty space. Right clicking anywhere in
> the gutter opens its menu, and the gap is where a folding arrow would go if Unluminous ever grows
> folding, so adding one later would not move the text.

So the arrow goes in that gap and **the gutter does not change width**, which means no screenshot in
the accepted set moves by a pixel except the ones that now have an arrow in them.

It is drawn rather than lettered, which is what `design/style-guide.md` asks for and what
`editor_view::box_rules` had to do for the Markdown tables: a chevron glyph is a whole pixel wider
than its advance and would not sit in the middle of a twelve point column. Two strokes, seven points
across, pointing **down** when the block is open and **right** when it is collapsed — which is what
IntelliJ, VS Code and every file tree in the world do, and which the explorer's own disclosure
triangles in Unluminous already do.

Quiet in `TEXT_FAINT`, and `TEXT_CONTROL` under the pointer or when the block is collapsed. A
collapsed block's arrow is never faint: it is the only thing on the screen saying that four hundred
lines are missing.

The gutter reports the click and decides nothing, which is what every component in Unluminous does:
`GutterOutcome::toggle_fold: Option<usize>` carries the paragraph.

### 7.2 The collapsed block

The head line stays. After the end of its text the window draws a small rounded rectangle in
`CONTROL` with three dots in it, and clicking it expands the block — which is IntelliJ's `{...}` and
Sublime's `⋯` and is the affordance a person reaches for before they think about the gutter.

It is painted by the window rather than put into the text, and that is a decision worth writing down.
Unluminous's Markdown preview is "a document, which is what makes it read like one": everything on the
screen is real text in a real `Document` and therefore selects, copies and hit-tests with no new
code. A placeholder is the opposite case. Putting `{...}` into the text would mean the layout
engine, the caret, the selection and the clipboard all had to know that three of the characters in
front of them are not in the file — which is the shadow-document problem of §1 in miniature. So it
is drawn over the line, it is not selectable, and copying the head line copies the head line.

**Copying across a fold copies the hidden text**, because the selection is a byte range in the real
document and the hidden bytes are inside it. That is what IntelliJ does, it is what a person means,
and it needs no code at all.

## 8. The commands, and what they are bound to

Seven actions, all of them parameterless and about the file that is showing, so all seven are
ordinary `Action`s with an arm in `run_action` — reachable from the menus, the keyboard,
`unluminous-cli action run` and the MCP server without any of them being taught anything.

| Action | What it does | Key |
|---|---|---|
| `ToggleFold` | Collapse or expand the innermost block the caret is in | `Ctrl/Cmd+Period` |
| `CollapseAll` | Collapse every region in the file | `Ctrl/Cmd+Shift+Period` |
| `ExpandAll` | Show all again | `Ctrl/Cmd+Shift+Comma` |
| `CollapseOthers` | Collapse everything that does not hold a marked passage | `Ctrl/Cmd+Alt+Period` |

`Ctrl+.` is IntelliJ's own key for folding the selection, so it is the one key a person who has used
one will already try; and the full stop is next to the comma that already opens Settings, which is a
small thing that makes a pair of them memorable. Nothing in Unluminous claims any of the four today, and
`app::action_names` fails the build if two menu entries claim one key equivalent, so this is checked
rather than believed.

**Where they are on the menus.**

- `View -> Folding`, a submenu holding all four, beside `Split`. It is on a real menu so that
  `unluminous-cli action list` — which is built by walking the real menus — finds them.
- The editing area's own right click menu, `components::text_menu`, gains `Collapse All But
  Highlighted` and `Expand All` under the highlight section, which is what the ticket asks for and is
  where a person who has just marked four passages is already pointing.
- The gutter's right click menu gains `Collapse All` and `Expand All`, because a person who has just
  right clicked the fold arrows is asking about folding.

They are **absent for a file that cannot fold**, which is Unluminous's rule for a control that can never
apply — the `F` button is not drawn for a `.rs` file and the view mode buttons are not drawn for a
`.txt` one. A picture has no folds. One function answers it, `folding_applies`, and the three menus
and the command line all ask it, so none of them can disagree.

### 8.1 Collapse All But Highlighted

The ticket's phrase, and it means the marked passages of `task-1663` — the colours behind a passage
that stay in the file and are written down beside the project. "The highlighted sections of the file"
is a thing a person has deliberately made, and folding everything else is the reading tool that
makes a set of marks worth having.

The rule is VS Code's `foldAllExcept`, stated over marks instead of selections:

> Collapse every region, then expand every region that holds a mark, and every region holding one of
> those.

Expanding the parents is what makes it work at all: a marked line inside a method inside a class is
visible only if the class and the method are both open.

**With nothing marked it falls back to the selection**, and with neither it says so in the status
bar rather than collapsing the whole file — which is what a person would see as the command having
gone wrong. Both are the same one rule, "keep what has been pointed at", and the fallback is written
down here rather than left to be discovered because a person who has selected a function and asked
for this plainly means that function.

## 9. The caret, and getting into a folded block

Two rules, and between them they are the whole of the interaction.

**A caret is never inside a hidden paragraph.** Anything that moves the caret into one expands the
folds around it first: `Go to Definition`, a search hit, `Find in Files`, `unluminous-cli editor caret
--line`, `Navigate Back`. It is one function, `reveal(offset)`, called from the one place a jump
lands — the same shape as `follow_the_open_file`, which is derived from the state rather than fired
from each of the eleven places that could need it, because the twelfth would be the one that forgot.

**A fold is expanded when the text inside it is edited.** In practice this cannot happen, because of
the rule above; it is asserted rather than arranged, so that a command line request that edits a
range inside a fold does not leave the file looking as though nothing happened.

Moving down from the head line of a collapsed block lands on the line after the block, because that
is the next line that exists. Nothing has to do that: `MoveDown` works through the layout, and the
lines in between are not in it.

## 10. The command line, and the agent

A new area, `fold`, because the areas are what the window is made of and this is a new part of the
window:

```
unluminous-cli fold list [--json]
unluminous-cli fold toggle [--line <number>]
unluminous-cli fold collapse [--line <number>] [--all]
unluminous-cli fold expand [--line <number>] [--all]
unluminous-cli fold others [--marked] [--selection]
```

`fold list` reports every region in the file that is showing — its head line, its last line, its
kind and whether it is collapsed — which is what an agent needs before it can ask for anything else,
and what makes the feature testable from outside the window.

The MCP tools come from the catalogue with no further work, which is the fourth rule of
`unluminous-cli/src/catalogue.rs` and the reason `unluminous_cli::mcp` exists:
`every_command_is_offered_as_a_tool_in_both_shapes` fails the build if one ever is not. The area's
title and note are written for somebody who cannot see the window, because that is who reads them.

Documentation is a test: `unluminous-cli/src/documentation.rs` fails while a command has no section in
`unluminous-cli/docs/commands.md`, and `cargo run -p unluminous-cli --example reference` writes it.

## 11. What it costs

The budget is `task-1666`'s: **nothing that runs once a frame may allocate**, and a frame costs what
is on the screen rather than what is in the file.

- The regions are worked out **once per text revision**, cached on the tab beside the symbols and
  keyed the same way, so a frame in which nothing was typed costs one integer comparison. They are
  read out of the same `syntax::scan` pass that colours the file.
- Resolving which paragraphs are hidden is a walk over the regions, which is a few hundred entries
  on the largest file in this repository, and it happens only when the folds or the text change.
- A frame with a fold open or closed costs one `relayout`, which is what typing a letter costs.
- `Hidden::contains` is a binary search, asked once per paragraph while laying out.

`cargo run --release -p unluminous-app --example folding_cost` prints the numbers that matter, so the
claim stays measured rather than remembered. On `crates/unluminous-app/src/app/mod.rs` — 274 kilobytes,
5,554 lines, the largest file in this repository:

| What | Cost |
|---|---|
| Reading its 1,276 blocks, with the tokeniser run for it | 4.57 ms |
| — of which the tokeniser alone | 3.03 ms |
| **Reading them with the scan shared, which is what the window pays** | **1.25 ms** |
| Collapsing every one of them, which is one `relayout` | 18.8 ms |
| A keystroke with a fold closed | 3.76 ms |

The third row is the one that matters and it is why the tokeniser is **shared rather than run
twice**: `colour_the_file` already scans the same text at the same revision, so it collects
`folding::Tokens` — the comments and the strings, which is the only thing reading brackets needs a
tokeniser for — and `fold_regions` uses them. Run separately it would have been 2.5 ms of every
keystroke on this file for a second answer to a question that had already been asked, which is
`task-1666`'s rule and `task-1681`'s fix restated.

A file past `UnluminousApp::COLOUR_LIMIT` is not read for its blocks at all, for the reason it is not
coloured: both are one linear pass over the text on every change. It keeps its line numbers and
loses its arrows.

## 12. Tests

**`unluminous-core`, with no window.**

- Every shape of block: a function, an `if`, a nested pair, `} else {`, `});`, a bracket inside a
  string, a bracket inside a comment, an unclosed bracket.
- Comments: a block comment over three lines, a run of line comments, a single line comment (not a
  region).
- Indentation: Python, YAML, a trailing blank line, a file that is all one level.
- Markdown: `#` over `##`, a heading inside a fence, a file with no headings.
- `Folds`: shifting under an insert and a delete above, inside and after; snapping to the head line;
  riding the undo snapshot.
- Layout: a hidden paragraph produces no lines and its band is `None`; the line numbers of the
  visible paragraphs are unchanged; `relayout` with a fold agrees exactly with `layout` with the same
  fold, for a fold at the start, in the middle, at the end, and for two folds at once. That last one
  is the same test `relayout_agrees_with_layout_after_every_shape_of_edit` already is, and it is what
  catches the byte-shift fault of §6.

**`unluminous-app`, with no window.** `folding_applies` for each kind of file; `Collapse All But
Highlighted` over a document with three marks, including the parent-expanding rule; the fold menus'
entries; `action_names`; the CLI catalogue round trip.

**Screenshot tests.** A source file with a block collapsed — the arrow, the badge, and the line
numbers jumping from 12 to 41. A file with everything collapsed. `Collapse All But Highlighted` over
a marked file. **Look at the images.**

**The real application.** `cargo run --release`, fold a function in `app/mod.rs`, check the line
numbers, edit above it, check the fold is still on the same function.

## 13. Deliberately not here

- **Recursive collapse and expand-to-level.** IntelliJ's `Ctrl+Alt+NumPad-` and `Ctrl+NumPad* 1..5`
  are for a person who lives in a hundred-thousand-line Java file. `Collapse All` and `Expand All`
  cover the ask; the nesting level is a dial with no reader.
- **Custom `//#region` markers.** They are a language configuration key in VS Code and would be a
  tenth manifest key here. Nothing has asked for one, and a fold that has to be typed into the file
  is a fold that lives in everybody's diff.
- **Folds that survive closing the file.** IntelliJ keeps them in `workspace.xml`. Unluminous could keep
  them in `.unluminous/` beside the marked passages, and this deliberately does not: a fold is a way of
  reading a file for a minute, and a file that opened with its contents hidden by a decision somebody
  made last Tuesday is a file that looks broken. The marked passages persist because a mark is
  deliberate and durable; a fold is neither.
- **Folding the Markdown preview.** The source pane folds by heading, which is the useful half. The
  preview is a rendering and folding it would mean a second fold state over a second layout.
- **Folding in a diagram or a picture tab.** There is nothing to fold.
