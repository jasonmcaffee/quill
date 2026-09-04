# task-1681 — deleting a file, saving a tab that is closed, and moving a file with its references

## 1. What was asked

> # Delete file option
> I should be able to press delete on a selected file, get a are you sure confirmation dialog.
> I should see a delete option in the right click options.
>
> # save on close
> If i close a tab that has been edited but not saved, it should save and close.
>
> # drag/drop files
> I should be able to move files to different locations.
> Moving them should do a refactor so anything that references them is updated.
> Do research online, create a tdd, and implement.  This is an important one.

Three asks that look unrelated and are not. All three are about the explorer and the tabs owning the
*files* rather than only showing them: a file can be thrown away, a tab can be shut without losing
what was typed into it, and a file can be put somewhere else without the code that names it going
stale. The third is the large one and the ticket says so.

Two of them also need something Unluminous has never had, and it is the same thing: **the explorer has to
be able to hold the keyboard.** `Delete` cannot mean "throw this file away" while the editing area
has the keys, because there it means "take away the letter in front of the caret". §5 is that piece,
and it is what makes the first ask an editor feature rather than a menu entry.

## 2. What the surveyed editors do

### 2.1 Deleting

**IntelliJ** puts `Delete...` on the project view's context menu with `Delete` as its shortcut, asks
a yes/no question naming what is about to go, and — where the platform has one — moves the file to
the system's trash rather than unlinking it. It also offers "safe delete", which searches for usages
first and refuses when there are any; that is a separate refactoring with its own dialog and is not
what this ticket asks for.

**VS Code** is the same shape with a different default: `Delete` on a selected explorer row, a modal
saying *"Are you sure you want to delete 'x'?"* with the reassurance *"You can restore this file from
the Recycle Bin"*, and `explorer.confirmDelete` to switch the question off. The reassurance is doing
real work: the question is easy to answer yes to precisely because the answer is recoverable.

**Sublime Text** deletes to the trash with a confirmation, and has done since version 2.

So: one question, one row on the menu, one key, and *to the trash where the platform has one*. That
last point is the one worth taking seriously — a confirmation is a much weaker safety net than a
Recycle Bin, and the two together are what every one of these ships.

### 2.2 Closing a tab that has been edited

Every editor surveyed asks. IntelliJ, VS Code and Sublime all put up a three-answer dialog — save,
don't save, cancel — and VS Code additionally keeps the buffer alive in "hot exit" so the question
can be dodged for the window as a whole.

The ticket asks for none of that: **"it should save and close"**. That is a deliberate, and
defensible, different answer, and it is the one Unluminous is better placed to give than the editors
above, because Unluminous saves plain text and nothing else. There is no format conversion, no "save as"
decision, no lossy round trip. Writing the buffer to the file it came from is exactly what the person
typed, and a dialog in front of it is a dialog that gets clicked through. It is also what a modern
note editor does — Obsidian, Bear, Apple Notes — and nobody misses the dialog.

The one case that has no honest answer is a tab with **no path**, and §6 says what happens to it.

### 2.3 Moving a file, and what happens to what names it

**VS Code.** Dragging a file in the explorer moves it. Whether the imports are then fixed is
`typescript.updateImportsOnFileMove.enabled`, which is `prompt` by default, `always` or `never`. The
work is done by the TypeScript language service, which type-checks the program, so it is a semantic
answer; it needs a `tsconfig.json`/`jsconfig.json` for the service to know what the program is, and
it will not touch anything in `node_modules`.

**IntelliJ.** Dragging a file between packages in the Project tool window *is* the Move refactoring —
the same thing `F6` opens — so references are updated by the same machinery that renames a class, and
dragging is simply the gesture that starts it. Holding the platform's copy modifier copies instead of
moving. The dialog reached by `F6` has a "Preview" button; the drag does not use it.

**rust-analyzer.** Implements the language-server `workspace/willRenameFiles` hook for single-level
file moves, which is how VS Code asks a server "I am about to move this — what edits do you want?".
It is honest about how far it goes: the tracked issues for moving a module into another directory,
for renaming a module's directory alongside its file, and for adapting references when code moves
between modules are all still open. Even the semantic tier finds Rust's module tree the hard part,
and the hard part is not the `use` lines — it is the `mod` declarations, which are the only thing
that makes a file a module at all.

Three things to take from all of this:

1. **Dragging in the file tree means "move and refactor"**, not "move" — IntelliJ has been the proof
   of that for twenty years.
2. **There is no single right spelling of a specifier**, which is why VS Code makes it a setting.
   Unluminous already decided this in `task-1680`: a relative specifier is the one that is always right.
3. **Say what tier you are on.** VS Code's is a type checker. rust-analyzer's is a compiler front end
   that still cannot do the whole job. Unluminous's is neither, and §4 says exactly what that buys and
   what it costs.

## 3. The shape of the answer

| Ask | Where it lives |
|---|---|
| The explorer holds the keyboard, has a selection, and is walked with the arrow keys | `components::explorer`, `app::Focus::Explorer` |
| `Delete` and a `Delete` row on the right click menu | `Action::DeletePath`, `services::recycle` |
| A modified tab is written when it is closed | `UnluminousApp::close_tab` |
| A row is dragged onto a folder | `components::explorer` reports it; the window settles it |
| Where a file's references are and what they should say | `unluminous_core::imports` reads, `services::file_move` decides |
| Doing it | `UnluminousApp::move_path`, which `Rename...` now goes through too |

## 4. The move refactor: which tier, and what that means

### 4.1 The tier is the one `task-1675` and `task-1680` already chose

`task-1675` §2 weighed a language server client, tree-sitter with stack graphs, and a syntactic index
built from the token stream Unluminous already produces, and chose the third. Every reason holds here and
one more is added: a move refactor that depended on a language server would work on a machine where
one happened to be installed and silently do nothing on a machine where one was not — and *silently
doing nothing* is the worst of the three possible outcomes, because the person has already dropped
the file.

So this is a **syntactic** refactor, and it rests on exactly two readings of the text, both of which
already exist in `unluminous_core`:

- For the **quoted** family — TypeScript, JavaScript, CSS — a module specifier is a string, and
  whether a given string *is* one is `unluminous_core::imports`' existing question, asked forwards over
  the file instead of backwards from a caret.
- For the **path** family — Rust — a module reference is a chain of segments, and where a file sits
  in the module tree is arithmetic over its path, its source root and its `import_index` names, all
  of which `services::imports` already computes for the completion popup.

**One reading, asked twice.** The rule `task-1675` set for the tokeniser — colouring a file and
reading its definitions are one reading of the rules rather than two that would drift — is what
decides where the new code goes. `imports::specifiers_in` is `imports::context_at`'s own test for
"is this string an import" run over every string in the file, so the popup and the refactor cannot
come to different conclusions about what an import is. Nothing new is taught about any language.

### 4.2 What it therefore does and does not see

It sees what is written down plainly. It does not see:

- **Macro-generated references**, in any language. A path assembled inside a macro is not in the
  text.
- **`#[path = "..."]`**, which lets a Rust module live at a filename that has nothing to do with its
  module path.
- **Bare package specifiers and alias tables** — `import { x } from '@app/thing'`. `task-1680` §12
  already ruled these out: `node_modules` is outside the walk, and a `tsconfig` alias table is a
  second resolver.
- **References written in a language with no `language.imports` key** — Markdown links to a moved
  file, a path inside `include_str!`, a path in a build script. §12 says what each would take.
- **Comments and strings**, deliberately, in the path family. A `use` path quoted inside a doc
  comment stays as it was, which is the same second-class treatment `task-1675` gives a textual
  match.

Everything it does not see is **reported by name**, in the same sentence that says what it did see. A
refactor that quietly did nine tenths of the job would be worse than one that did none, because
nobody would look.

### 4.3 Why there is no preview modal, and why that is not the same decision as rename

`task-1675` gave `Rename Symbol` a preview with a tick box on every row, and stated the reason: on a
syntactic tier the person's confirmation is the correctness mechanism. That argument is about
**names**, and it does not carry over here, for three reasons.

**A name is ambiguous and a path is not.** Renaming `new` has to decide which of forty `new`s are the
same `new`; the tick boxes exist because the mechanism genuinely cannot tell. A module specifier
resolves to one file or to no file, and one that resolves to no file is left alone. There is nothing
for a person to disambiguate.

**The move is its own inverse.** Drag the file back and the same arithmetic runs the other way and
puts every specifier back exactly as it was, because the specifiers were derived from the paths in
the first place. There is no equivalent for a rename — renaming back is a second guess at the same
ambiguity.

**A gesture that opens a modal is a gesture nobody uses.** IntelliJ has both, and its drag does not
open the preview. Dragging a file is a file-manager motion; putting a dialog in front of it turns
half a second into five.

What replaces the preview is a **report**: the status bar says what moved, how many references were
rewritten in how many files, and — in the same sentence — every file that was skipped and why.
`unluminous-cli explorer move --dry-run` prints the whole change set without touching anything, which is
the preview for anybody who wants one, and is what a test asserts against.

### 4.4 The ownership rule is `task-1675`'s, unchanged

*A file that is open is owned by its `Document`, and every other file is owned by the disk.*

- An **open** file's references are read from its live text and rewritten as one
  `Command::ReplaceMany` — one undo step by construction — and the tab is left **modified rather than
  written**. A refactor must never silently write a buffer somebody was editing. (The one thing that
  *does* now write such a buffer is closing its tab, which is §6, and which is the person saying so.)
- A **closed** file is read, every range is checked to still hold what the plan expected, and only
  then is it written once. A file that changed underneath the plan is skipped whole and named.
  `services::file_marks` shifts that file's stored marks by the same edits, exactly as
  `symbols::rewrite_closed_file` does, because this is the second place a closed file's bytes move.

### 4.5 The two families

#### Quoted — TypeScript, JavaScript, CSS

For every file `F` in the project whose language has `language.imports = quoted`:

1. `imports::specifiers_in(text, grammar)` gives every module specifier written in `F`, with the byte
   range of its content.
2. Each is resolved with `services::imports::resolve_specifier` against the project **as it is before
   the move**. A specifier that resolves to nothing is left alone — it is a package, a `://` address,
   or a mistake, and none of the three is this refactor's business.
3. The target's path **after** the move and `F`'s own path after the move are both known, so the
   specifier that *should* be written is `specifier_for(F_after.parent(), target_after, grammar)`.
4. If that differs from what is written, it is an edit. If it does not — which is every specifier in
   a folder that moved as a whole — there is no edit, and that is the case that makes moving a folder
   cheap rather than a hundred pointless rewrites.

Two details that stop it churning. **The written form's shape is kept**: a specifier written with its
extension keeps it even where `import_omit_extension` would drop it, because rewriting `'./a.js'` as
`'./a'` is a change nobody asked for. And a specifier that resolves to a folder's `index` file keeps
being written as the folder, because `specifier_for` already answers that way.

#### Path — Rust

A file's **module** is arithmetic: its nearest ancestor named by `language.source_roots` is the
source root, the package is that root's parent folder with `-` read as `_`, and the segments are the
path from the root down, with a final `import_index` stem (`mod`, `lib`, `main`) dropped. So
`crates/unluminous-app/src/services/file_clipboard.rs` is `unluminous_app` + `["services", "file_clipboard"]`,
and `crates/unluminous-app/src/services/mod.rs` is `unluminous_app` + `["services"]`.

Moving the file changes those segments, and three kinds of text have to follow.

**`use` statements.** Each is parsed into its leaves — `use crate::a::{b, c as d, e::*};` is three —
because a leaf is the unit that can move out from under a shared prefix. A leaf's anchor is resolved
(`crate` is the referencing file's own package, `self` and `super` walk its own module, anything else
is looked up among the project's packages), giving an absolute module path. If that path starts with
the moved module's old absolute path, the leaf needs rewriting to the new one. **A leaf that already
resolves correctly after the move is left alone**, which is what keeps a folder's internal
`super::sibling` references untouched when the whole folder moves.

The rewrite is minimal where it can be: when every leaf of a statement shifts by the same written
prefix, only the prefix bytes are replaced, so `use crate::services::file_clipboard::free_name;`
becomes `use crate::components::file_clipboard::free_name;` and nothing else about the line moves.
When only some leaves of a group move, the statement is **re-emitted** from its leaves, grouped by
common prefix — `use crate::services::{file_tree, file_clipboard};` becomes two lines. There is no
third case.

**Qualified paths outside `use`.** `services::file_clipboard::free_name(&folder, &name)` in the body
of a function is the same chain without the keyword, and is rewritten by the same prefix
substitution. These are found by scanning the token stream — so a path inside a comment or a string
is never touched — and chaining words joined by `language.path_separator`.

**The `mod` declarations, which are the part that actually breaks the build.** A Rust file is not a
module because of where it sits; it is a module because some other file says `mod name;`. So the move
also:

- finds the declaration in the **old** parent module file — `mod x;`, `pub mod x;`, `pub(crate) mod
  x;` — and takes it out, along with any `#[…]` attribute or `///` doc comment lines directly above
  it, because those belong to the declaration and not to whatever follows it;
- puts the same declaration, with the same visibility, into the **new** parent module file, in
  alphabetical order among the declarations already there, or after the file's leading `//!` and
  `use` block when there are none;
- and, when the destination folder has no module file at all, **says so** rather than inventing one.
  Making `mod.rs` where there was none is a decision about the shape of somebody's crate, and this
  refactor does not get to make it.

`crate::` and the package spelling are what a rewritten path is written as, even where the original
was written `super::`. That is a real change of style in a small number of lines, and it is the price
of a rule that is always right rather than usually right: re-relativising a path correctly needs to
know where the *reader* is as well as where the target is, and gets it wrong at exactly the moments
it matters.

### 4.6 Moving a folder

A folder move is the same plan with more entries: every file under it is a `(from, to)` pair, and
everything above works on the list. In the path family the folder itself is a module, so its own
declaration is the one that moves; the files under it keep their relative module positions and
therefore mostly need no edits at all, which §4.5 relies on.

## 5. The explorer takes the keyboard

`Focus` gains a third value. Today it is `Editor` or `Terminal`; it becomes `Editor`, `Explorer` or
`Terminal`, and `UnluminousApp::selected` holds the path the explorer's own cursor is on.

**What a click means.** A single click on a row selects it, opens the file in the tab a single click
reuses, and leaves the keyboard **in the explorer** — which is IntelliJ's behaviour and is what makes
`Down` `Down` `Down` a way to look through a folder. A double click opens the file permanently and
gives the keyboard to the **editor**, which is where somebody who double clicked is going. A right
click selects without opening. Clicking in the editing area or the terminal takes the keyboard back,
exactly as it does today.

**And a letter typed hands the keyboard straight to the editor.** This is the rule that makes the
paragraph above safe rather than annoying. Clicking a file in the tree and then typing is one of the
commonest things anybody does in an editor, and a tree that swallowed the first word would be a
regression however well the ring signposted it. The explorer has no use for a printable character —
its filter box is a field of its own — so any letter means the editor, and the handover happens
before any pane reads the frame's input, which is what makes the letter that caused it land in the
document. Nothing is guessed: `Delete` and the arrow keys produce no text event and stay the
explorer's.

**What the keys do, and only while the explorer has them.** `Up` and `Down` move the selection
through the rows that are showing — the same list the explorer draws, so a row inside a shut folder
is not stepped onto. `Right` opens a folder, `Left` shuts it or steps to its parent. `Enter` opens
the selected file permanently and hands the keyboard to the editor. `Escape` hands the keyboard back
without opening anything. `Delete` asks the question in §7 — and so does the command key with
`Backspace`, which is IntelliJ's own answer for the Mac keyboard that has no `Delete`. **`Backspace`
on its own deliberately does nothing**, because it is exactly the key somebody who has just clicked
a file is about to press in the editor, and a delete confirmation is not the right answer to it.

**How the selection is drawn.** The pill the open file already has, in `SELECTED_ROW`; the selected
row additionally gets a one point `ACCENT` ring **only while the explorer has the keyboard**, so
there is never a doubt about where a key press is going. A row that is both is a filled pill with a
ring, which is the ordinary case of clicking a file and is exactly what it should look like.

## 6. Saving a tab that is closed

`UnluminousApp::close_tab` is the one place a tab is closed — the tab's cross, `Ctrl+W`, the tab menu, the
command line and the rename that reopens a tab at a new path all reach it — so this is one change in
one function.

Before the tab is taken away: if its document is **modified** and it **has a path**, it is written.

Three cases are deliberately excluded, each with a reason:

- **A picture tab** holds an empty document over the picture's path, so writing it would put nothing
  over the picture. `save` already refuses for this reason and so does this.
- **A tab with no path** has nowhere to be written and choosing one is a dialog, which is the thing
  this ask is removing. It is closed as it always was, and the status bar says that it was — an
  untitled scratch buffer being written into the project as `untitled.md` because somebody shut it is
  litter nobody asked for.
- **A tab whose file is being deleted**, which closes it on the way past. Writing a file in order to
  throw it away is not a thing to do.

The status bar says what was written, because a save nobody asked for out loud is a save that has to
announce itself.

## 7. Deleting

`Action::DeletePath(PathBuf)`, on the explorer's right click menu under `Rename...` with `Delete`
shown beside it, and on the `Delete` key when the explorer has the keyboard.

**It asks first**, through the one confirmation Unluminous already has. To do that, `UnluminousApp::Confirmation`
stops holding a git request and starts holding an `Answer`, which is either a git request or a path to
delete. That is one enum and two arms; the alternative is a second confirmation dialog that almost
agrees with the first, which is the thing `components::modal` exists to prevent.

The question names what is about to go and where it is going: *"Delete notes.md. It goes to the
Recycle Bin, so it can be got back."* for a file, and for a folder it counts what is inside —
*"Delete services and the 14 files in it."* — because the count is the fact that changes the answer.

**Where it goes.** `services::recycle` is the one function that answers that.

- On **Windows** it is the Recycle Bin, through `SHFileOperationW` with `FOF_ALLOWUNDO`. That is one
  feature flag on the `windows-sys` dependency Unluminous already has for the window's transparency, and
  no new crate.
- **Everywhere else** it is `std::fs::remove_file`/`remove_dir_all`, and the question says so: *"This
  cannot be undone."*

That divergence is a stated cost rather than a hidden one. The `trash` crate would close it and
brings a dependency tree per platform; macOS's own `NSFileManager.trashItemAtURL` would close it with
no new crate and cannot be compiled, let alone tested, on the machine Unluminous is built on.
`Destination` is an enum with two values and the dialog's wording is derived from it, so the day the
second platform is answered there is one place to change and the sentence follows.

**Afterwards**: every tab on a deleted file — or on any file under a deleted folder — is closed, the
tree is read again, the project's marks for those paths are forgotten, and the symbol index is told
the project changed. A tab closing here does **not** save first, for the obvious reason.

## 8. Dragging

The explorer's rows already sense `click_and_drag`, so the gesture costs nothing to start. What it
needs is somewhere to land.

**The component reports and decides nothing**, which is Unluminous's rule for a component and is the same
shape `task-1673` gave the tab drag. While a row is being dragged the explorer collects every row's
rectangle and path as it draws them, and after the list is drawn it works out which row the pointer
is over. A folder row is that folder; a file row is the folder the file is in, which is what IntelliJ
does and is what somebody aiming at a crowded folder means. The heading is the project root. The
target folder is drawn with the hover fill and the carried file's name follows the pointer.

Three things it refuses, silently, by not offering a target: a folder dropped into itself or into
anything under it, a path dropped into the folder it is already in, and anything dropped outside the
explorer — the last so a drag can be thought better of, exactly as a tab drag can.

**What the window does with it** is `UnluminousApp::move_path(from, to)`, which is §4 and which
`Rename...` now calls as well — renaming a file *is* moving it to a new name, and a rename that
updated no references while a drag did would be two answers to one question.

The move itself is `std::fs::rename`, falling back to copy-then-remove when the two are on different
volumes, which is what `services::file_clipboard` already does for a paste.

## 9. The command line

Everything above is reachable, because `task-1661` says it has to be.

| Command | What it does |
|---|---|
| `unluminous-cli explorer select [path]` | Set the explorer's selection, or print it |
| `unluminous-cli explorer delete <path>` | Delete, without the question — a command line *is* the deliberate act |
| `unluminous-cli explorer move <path> <folder>` | Move it, and rewrite what refers to it |
| `unluminous-cli explorer move <path> <folder> --dry-run` | Print the whole change set and change nothing |
| `unluminous-cli explorer move <path> <folder> --no-refactor` | Move the bytes and leave the references alone |
| `unluminous-cli tab close --discard` | Close without the save §6 added |

`action run delete-path --path x` reaches the same action the menu row does, because the names are
walked out of the real menus.

## 10. What is tested

**Pure, with no window** — `unluminous-core`:

- `imports::specifiers_in` finds the specifier in each of `import x from './a'`, `import './a'`,
  `import { a } from "./a"`, `export * from './a'`, `require('./a')`, `import('./a')` and `@import
  'a.css'`, and finds **nothing** in an ordinary string, a string on the line after an import, or an
  import inside a line comment. These are the same cases `context_at` already has, run the other way
  round, which is the point.
- `imports::use_statements_in` reads `use a::b;`, `use a::b as c;`, `use a::{b, c};`, `use a::{b,
  c::{d, e}};`, `use a::*;`, `pub use a::b;` and a statement written over four lines, and reports the
  leaves and the statement's own byte range.
- `imports::paths_in` finds `a::b::c` in code and not in a comment or a string.

**Pure, with no window and no disk** — `services::file_move::plan` against an in-memory project (the
file list and a closure that answers with each file's text):

- a TypeScript file moved between folders rewrites the importers and its own relative imports;
- a file moved into the folder it already imports from ends up with `./x` rather than `../a/x`;
- a folder moved whole rewrites the importers **outside** it and leaves every specifier inside it
  alone;
- a specifier written with its extension keeps it; one written as a folder stays written as a folder;
- a Rust file moved between modules rewrites `crate::`, the package spelling, and a `super::` that no
  longer resolves, and leaves alone a `super::` that still does;
- a grouped `use` with one moved member is split into two statements and the others are untouched;
- the `mod` declaration is taken out of the old parent and put into the new one in alphabetical
  order, with its visibility and its attributes;
- a destination folder with no module file produces a **note**, not an edit.

**With a window** — `crates/unluminous-app/tests/screenshots.rs`:

- the explorer's menu holds `Delete` and running it opens the confirmation, which names the file;
- confirming it takes the file off the disk and closes the tab that was on it;
- the arrow keys walk the selection and the ring is drawn on the row the keyboard is on;
- `Delete` with the explorer focused deletes, and `Delete` with the editor focused still edits the
  text — the two must never both fire;
- closing a modified tab writes it, and closing an untitled one does not;
- a move through `unluminous-cli explorer move` rewrites a real importing file on disk and leaves an open
  importing tab modified but unwritten;
- `--dry-run` changes nothing at all.

**A picture** for the explorer with a selection ring, because a ring nobody has looked at is not a
design.

## 11. Performance

Nothing here runs once a frame. The plan is built once per move, and its cost is one reading of every
file in the project that has an import-capable grammar. `unluminous-cli explorer move --dry-run` prints
the elapsed time, so it stays measured rather than assumed.

Measured against Unluminous's own repository — 1,001 files in the walk — planning the move of a Rust
module file is **62 ms** and planning the move of a whole module folder, which is 56 references in
37 files, is **56 ms**. That is a plan built on the frame the row is dropped, and at that size it is
one frame's worth of work rather than a pause anybody sees.

It was **557 ms** when it was first written, and the fix was removing waste rather than capping
anything. Two faults, both of the kind `task-1666` catalogued. `syntax::scan` was being run five
times over every file, because each of `use_statements_in`, `paths_in` and `nesting` read the tokens
for itself; they take one `imports::Tokens` now, which is the reading done once. And `nesting`
looked for the comment or string starting at each byte by **searching the list** — quadratic in the
size of the file — where the list is already in order and an index into it is enough.

The one thing that *is* on a frame is the drag: while a row is being carried the explorer keeps a
`Vec` of the rectangles it drew, which is the rows that are showing and no more.

## 12. Deliberately not in this ticket

- **Auto-import on completion.** Still `task-1680` §12's decision, unchanged.
- **`tsconfig` path aliases and bare specifiers.** Same.
- **Markdown links to a moved file.** Markdown has no `language.imports` key and giving it one would
  mean a third family — a link is neither a quoted module nor a path of segments. It is the obvious
  next one and it would be a small ticket.
- **Safe delete.** IntelliJ's "find usages first and refuse if there are any" is a different feature
  from the one asked for, and the machinery this ticket adds is most of what it would need.
- **Copy on drag with the platform's modifier.** IntelliJ has it; nobody asked, and `Copy`/`Paste`
  already do it from the menu.
- **Auto-scrolling the tree while dragging past its edge.**
- **Multiple selection**, and therefore moving or deleting several files at once.
- **Undo for a move.** Dragging it back is the undo, and it really is one — §4.3 says why.
- **Saving every modified tab when the window closes.** It is the same rule as §6 and it belongs with
  a decision about what Unluminous does on quit generally, which is a ticket of its own.
