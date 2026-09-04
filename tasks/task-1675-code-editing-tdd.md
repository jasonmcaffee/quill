# task-1675 — go to definition, find all references, and rename

## 1. What was asked

> Do deep online research on how to do function, param, class, etc refactoring, CMD+Click to jump
> to definitions, and find all references.
>
> Find all references should open a modal that has the file path, then under that scrolled to the
> first reference in that file, with the reference highlight.
>
> Create a tdd to create a highly optimized, extremely performant mechanism to handle these, that
> works flawlessly, and is battle tested through ample scenarios that are covered in the tdd.

Three features, and they are one mechanism wearing three faces: something that, given a name in a
file, can say where that name is defined and where it is used. Go to definition asks the first
question, find all references asks the second, and rename is the second question followed by an
edit. The research below is about the only decision that matters — how much the editor should
*understand* — and everything after it follows from the answer.

"Refactoring" here is read as **renaming** a function, a parameter, a class, a variable — the
identifier-shaped refactorings. Change-signature, extract-function and their relatives are not
asked for and are not designed here; §12 says so explicitly.

## 2. The three ways of knowing what a name means

Every editor that has these features sits on one of three mechanisms, and the research looked at
all three in depth: acting as a Language Server Protocol client, embedding a parser (tree-sitter,
or GitHub's stack graphs on top of it), or a syntactic index built from the editor's own reading
of the source. The short version of a long survey:

### 2.1 A language server client, weighed and rejected

LSP is how VS Code, Helix, Zed and Kakoune answer all three questions, and it gives the true
answer: rust-analyzer knows that `self.count` and a local `count` are different things. The
protocol surface a minimal client needs is well understood — `initialize`/`initialized`,
`didOpen`/`didChange`/`didClose`, the three feature requests, and `WorkspaceEdit` coming back from
a rename — and the traps are documented: positions count UTF-16 code units unless a different
encoding is negotiated; three server-to-client requests (`workspace/configuration`,
`client/registerCapability`, `window/workDoneProgress/create`) deadlock a server that is never
answered; a `WorkspaceEdit` must be applied back to front and checked against document versions.

It was rejected for this task, for reasons that are Unluminate's own rather than the protocol's:

- **The feature dies on most machines.** A server is a separate program per language —
  rust-analyzer, typescript-language-server, pyright — that has to be installed, found on `PATH`,
  and fed its project file (`Cargo.toml`, `tsconfig.json`, `compile_commands.json`). Helix's answer
  to a missing server is a health command that says so; Zed's is downloading binaries from the
  network. Unluminate fetches nothing, ever.
- **The costs are out of scale with the product.** rust-analyzer typically holds one to four
  gigabytes and has been measured at twelve on dependency-heavy workspaces; tsserver has shown
  twenty-second stalls while a monorepo loads. Unluminate's whole editor budget is a frame at sixty a
  second.
- **Nothing about it can be a screenshot test.** When a server answers, and with what, depends on
  the machine, the server version and the phase of its indexing. Every layout in Unluminate is
  deterministic so that a picture can be a test; a feature that cannot be tested that way would be
  the first.
- **The tokeniser's own comment already made this choice**: "Real understanding is a language
  server, and that is not what this is." The design below keeps that sentence true.

What the survey of LSP clients *did* contribute is the client-side shape every good implementation
converges on regardless of mechanism — resolve before the click so the underline is honest, cancel
superseded requests with a generation counter, apply multi-edits as one undo step per document,
show the mechanism's own refusal message — and §4–§6 use all of it.

### 2.2 Tree-sitter and stack graphs, weighed and rejected

Tree-sitter parses incrementally and tolerantly (a 10,000-line C file in under 100 ms cold,
sub-millisecond after an edit), and its `locals.scm` queries resolve lexical scope correctly
within one file. But a grammar is generated C — the TypeScript one compiles to hundreds of
kilobytes — and every editor that ships tree-sitter either compiles grammars on the user's machine
(Helix, Neovim, a recurring support burden) or runs them as WebAssembly under a sandbox (Zed,
which is a runtime Unluminate deliberately does not have). A tree-sitter grammar is code, and Unluminate's
plugins are data; bundling a fixed set into the binary is the mermaid precedent, but it would also
be a second, parallel reading of every language beside the tokeniser that already reads them.

Stack graphs — GitHub's Rust crate for precise cross-file name resolution without a type checker —
are the right theory and the wrong bet: rules exist for four languages, each was written by
experts over weeks, and the repository was archived in September 2025 with a note to fork it if
you want it. GitHub itself kept its tree-sitter-tags fuzzy tier as the fallback for everything
else.

### 2.3 A syntactic index, chosen

The third mechanism is the one Unluminate already half-has. Sublime Text's goto-definition is its
syntax definitions feeding a symbol index — no resolution, a candidate list when a name is
ambiguous — and it is well liked because it is instant and predictable. GitHub's shipped
"search-based" code navigation is the same tier: all same-named definitions plus all same-named
references, for twenty-four languages, on every repository under 100,000 files, zero
configuration. Microsoft's `vscode-anycode` re-implements exactly this tier for environments
where no language server can run. It is a shipped, respected product tier, not a stopgap.

Unluminate's version of it: **definitions are read from the token stream the tokeniser already
produces, driven by per-language data in the plugin manifest; references are the project text
search that already exists, narrowed to whole words and classified by the tokeniser; rename is
that search feeding a preview the user confirms.** No new parser, no process, no network, nothing
executed from a plugin, every piece testable with no window, and every answer honest about being
syntactic: where the mechanism cannot tell two same-named things apart, it says so by showing
both, never by silently guessing one.

The one architectural courtesy paid to the future: everything the window asks for goes through two
questions — "where is this defined" and "where is this used" — expressed as plain functions in one
module. A later semantic tier (an LSP client, if the product ever wants one) would answer the same
two questions through the same seam. Qt Creator's tiering — a fast approximate answer now, a
precise one when a heavier index exists — is the pattern, and the seam costs nothing today.

## 3. The mechanism

### 3.1 `unluminate_core::symbols` — definitions are read from the tokens

A new module beside `syntax`, and like `syntax` it draws nothing and its tests run with no window.
It makes three things from a file's text and its `Grammar`:

```rust
/// What kind of thing a definition names, for ranking and for the modal's label.
pub enum SymbolKind { Function, Type, Constant, Variable, Module }

/// How sure the mechanism is. Sure came from a definer keyword; Likely from a shape heuristic.
pub enum Confidence { Sure, Likely }

/// One definition: the identifier's byte range, its kind, and how it was found.
pub struct Definition { pub name_range: Range<usize>, pub kind: SymbolKind, pub confidence: Confidence }

/// Where an identifier occurrence sits in the file's reading: real code, or inside a comment or
/// string, which every rename and reference list treats differently.
pub enum Role { Code, Comment, String }
```

- `file_definitions(text, grammar) -> Vec<Definition>` — one linear pass over the tokeniser's
  output. The rule is the same shape as the tokeniser's own: **a definer keyword followed by a
  word makes that word a definition** of the kind the grammar assigned to the keyword. `fn draw`
  defines `draw` as a function; `struct Layout` defines `Layout` as a type; `const LIMIT` defines
  `LIMIT` as a constant. The pass skips nothing clever: tokens classified `Comment` or `String`
  can never contain a definition, because the tokeniser already said what they are.
- `identifier_at(text, offset, grammar) -> Option<Range<usize>>` — the word under a point, grown
  in both directions by `Grammar::is_word_character`, so `--brand-hue` is one identifier in CSS
  and `mid-word` is two words in Rust. Returns `None` when the offset sits in a token classified
  `Comment` or `String`, or on a keyword, an operator or a number: a click on `return` is not a
  question about a symbol.
- `occurrences(text, name, grammar) -> Vec<(Range<usize>, Role)>` — every whole-word occurrence of
  `name`, each labelled with the role of the token it fell inside. Whole-word means bounded by
  characters that are not word characters *for this grammar*, which is what stops `count` matching
  inside `counter` and lets `-` bound a word in Rust while being inside one in CSS.

All three are pure functions over `(text, grammar)`. Nothing in them knows about files, threads or
panes.

### 3.2 The grammar keys a language adds

Two new manifest keys, both off unless a language asks — the same opt-in rule
`word_characters`, `types` and `hex_colors` already follow, kept by the same style of test
(`the_older_plugins_ask_for_none_of_what_the_symbols_added`):

```
language.definers          = fn=function, struct=type, enum=type, trait=type, mod=module,
                             const=constant, static=constant, type=type, let=variable
language.brace_definitions = true
```

`language.definers` is a comma list of `keyword=kind`. The bundled plugins gain:

| Plugin | `language.definers` | `brace_definitions` |
|---|---|---|
| rust | `fn=function, struct=type, enum=type, trait=type, mod=module, const=constant, static=constant, type=type, let=variable` | off |
| javascript | `function=function, class=type, const=variable, let=variable, var=variable` | on |
| typescript | the JavaScript list plus `interface=type, enum=type, type=type, namespace=module` | on |
| css | none | off |
| mermaid | none | off |

`language.brace_definitions` is the one heuristic, and it exists for the definition Rust never
hides but JavaScript and TypeScript do: **a class method has no keyword in front of its name**.
The rule: a token the tokeniser classified `Function` (a word directly before `(`), not preceded
by a `.`, whose parameter list closes on the same line and is followed by `{`, is a **Likely**
definition of kind function. `render(area) {` is caught; `if (ready) {` is not, because `if` is a
keyword and never a `Function` token; `list.map(x => {` is not, because `map` follows a dot;
calling `draw(area)` is not, because no `{` follows the `)`. A method whose parameters span lines
is missed, and that is stated in the plugin's `plugin.limitations` rather than half-fixed: a
`Likely` tier that guesses harder stops being honest.

CSS deliberately gets no definers. `--brand-hue: 280` defines a custom property by position, not
by keyword, and a rule that read `:` as a definer would call every property a definition. Find all
references still works for CSS — `occurrences` needs no definitions — and go to definition is
simply absent for a `.css` file, which is Unluminate's rule for a control that cannot apply.

### 3.3 The definitions index, and who owns a file's entry

`services::symbol_index` holds the project's definitions in memory:

```rust
name (interned) -> Vec<(file_id, name_range, kind, confidence)>
```

- **Built on a thread, never where the window draws.** The same arrangement as
  `services::text_search`: a worker with a request channel, a generation `AtomicU64` so a rebuild
  overtaken by a newer one stops where it is, and a waker that asks the window to repaint. The
  file list is `FileTree::all_files` — the same list `Go to File` and `Find in Files` search, with
  `target`, `node_modules` and `__pycache__` already left out — and only files whose extension has
  a grammar with a `definers` list (or `brace_definitions`) are read at all.
- **Cost, measured against what exists**: tokenising is 1.4 ms for a coloured 170 KB file and the
  whole-project text search reads 618 files in 20 ms on Unluminate's own repository, so a full index
  build is tens of milliseconds of reading plus tens of tokenising — well under half a second cold,
  on the thread, with the window drawing throughout. The index itself is small: a name, a range
  and two discriminants per definition, a few hundred kilobytes for a project this size.
- **One rule decides staleness, and it is the highlights' rule**: *a file that is open is owned by
  its `Document`, and every other file is owned by the index.* An open document's definitions are
  computed from its live text, cached on the tab keyed on `Document::text_revision()` — the same
  key `colour_the_file` uses, so an edit that does not change the text recomputes nothing. The
  disk-owned side is refreshed per file when Unluminate saves one, and rebuilt when a project opens.
  A file changed outside Unluminate can therefore be briefly stale, and §4.2 shows why that cannot
  put a jump in the wrong place: the range is re-checked at the moment of use, exactly as
  `open_the_match` already re-checks a search hit.
- **Lookups allocate nothing.** The hover query (§4.1) runs while the pointer moves, so the index
  answers `definitions_of(&str)` by hash lookup against interned names, and the per-document
  occurrence list is a sorted `Vec` binary-searched by offset — the `StyleSpans::spans()` lesson
  applied to a new table.

### 3.4 References are a search, not a table

Find all references does **not** read a stored inverted index of every identifier occurrence in
the project. It runs the machinery `Find in Files` already trusts: the generation-cancelled worker
thread walks `all_files`, reads fresh text from disk (so the answer is never stale), and for each
file that contains the word at all — a plain substring test first, because most files do not — runs
`symbols::occurrences` to keep only whole-word matches and label each with its `Role`. Results
stream into the modal in batches as files are read, exactly as `Find in Files` results do, with the
same honest cap (`LIMIT`, labelled "the first N — there are more") if a name is pathologically
common.

Why not store all occurrences? Because the stored copy buys nothing this project size can feel —
20 ms is already under the 100 ms threshold where an answer feels like the user's own action — and
it costs the one thing the search never has to pay: invalidation. A file edited outside Unluminate, a
branch switched under the window, a generated file rewritten by a build — the search reads what is
on the disk now, and an index of every occurrence would have to notice. rust-analyzer itself
answers find-usages as "text search, then check each candidate", for the same reason.

An open document is the exception here as everywhere: its occurrences come from its live text, so
references in a file with unsaved edits are the edits' truth, not the disk's.

### 3.5 Ranking, and being honest about ambiguity

A syntactic index will sometimes hold several definitions for one name. The order candidates are
shown, and which one a plain click jumps to:

1. definitions in the **same file**, the nearest one *above* the click first — which is what makes
   a shadowed local resolve to the nearest `let` above it more often than not, without pretending
   to scope analysis;
2. then definitions in other files, `Sure` before `Likely`, functions and types before variables,
   then by path order for determinism.

One candidate: jump. Several: the references modal (§5) opens listing the definitions, because a
picker for "which `new` did you mean" and a reference list are the same furniture. Zero: §4.4.
Nothing ever silently jumps to a guess when the mechanism knows it guessed — that is the line
between this tier and a bad IDE.

## 4. Ctrl/Cmd+Click, and the jump

### 4.1 The underline is resolution-driven

While the platform's modifier is held (`command` on macOS, `ctrl` on Windows — egui's
`Modifiers::command` is already both) and the pointer is over the editing area, the window asks
`identifier_at` for the word under the pointer and the index for its definitions. Only when a
definition exists is the word underlined and the cursor a pointing hand; a word the index knows
nothing about gets no affordance, so the promise the underline makes is one the click can keep.
This is VS Code's model — resolve on hover, before any click — and it is what makes the click feel
instantaneous: the answer was already in hand.

Per frame this is one `identifier_at` over one line and one hash lookup, cached against
`(text_revision, word_range)` so a pointer resting still costs nothing. The underline is drawn by
the editor's painter in the accent colour under the word's glyph range; no new widget, no layout
change.

The click handler sits in `editor_view::handle_pointer`, which already sees every click: modifier
held and a definition resolved → the component reports a `JumpRequest` in its outcome instead of
placing the caret, and the window acts on it — the same "components report, the window decides"
rule every component follows. A modifier-click on a word with no definition places the caret as an
ordinary click, having shown no underline.

### 4.2 Landing

`UnluminateApp::open_the_match` already does everything a jump needs: opens the file as a real tab,
refuses a range that has drifted past the end of an edited document, selects the range, scrolls
the caret into view and gives the editor the keyboard. The jump to a definition is
`open_the_match(path, name_range)` — and before calling it on a disk-owned candidate, the window
re-reads the target's current text and confirms the recorded range still holds the expected name,
re-finding it by `file_definitions` if it moved. A stale index entry therefore costs one file read
at click time and can never land a jump on the wrong bytes.

The selection *is* the landing highlight: it is how `Find in Files` already shows a match, it is
visible in every theme, and it clears the moment the person moves the caret — which the research
found is exactly the behaviour of VS Code's `symbolHighlightBackground` (persist until the caret
moves; nobody fades on a timer).

### 4.3 Already at the definition

A modifier-click *on* a definition pivots to the references modal for that name — IntelliJ calls
the whole command "Go to Declaration or Usages", and VS Code's providers do the same. Going to a
definition from the definition has no other meaning, and the pivot is what makes one gesture serve
both directions of the question.

### 4.4 A miss says so

`Go to Definition` invoked from the menu or keyboard on a word the index has nothing for shows
`No definition found for 'name'` in the status bar — the same channel every git message uses, and
the mechanism's own honest answer rather than an invented one. The modifier-click path rarely gets
here, because no underline was shown.

### 4.5 Navigate back

Half of the gesture. The window keeps a small bounded stack of `(path, caret offset)`; every jump
(definition, or opening a reference from the modal) pushes where the caret *was*, and
`Navigate Back` pops it, reopening the tab if it was closed. A stack of sixty-four entries, on the
window, not persisted: it is travel history, not state. `Navigate Forward` is the mirror stack,
pushed by `Navigate Back` and cleared by any new jump.

### 4.6 Menus and keys

Four new `Action` variants, entries in the list `app::actions::menus` builds, and arms in
`run_action` — which puts them in both menu bars and, through `unluminate-cli action list`, on the
command line for free:

| Entry (Edit menu, under `Find in Files`) | Key |
|---|---|
| `Go to Definition` | `Ctrl/Cmd+B` (and the modifier-click) |
| `Find References` | `Alt+F7` |
| `Rename Symbol` | `Shift+F6` |
| `Navigate Back` / `Navigate Forward` | `Ctrl+Alt+Left` / `Ctrl+Alt+Right` |

IntelliJ's keys, because Unluminate's search modals already chose IntelliJ's. The existing menu test
that refuses two entries one key equivalent guards the additions on macOS. The three symbol
entries are **absent** — not dimmed — when the active file's grammar has no definers and no
`brace_definitions` (for rename: when the file has no grammar at all), answered by one new
function in `services::file_kind` beside `formatting_applies` and `preview_applies`, so the menu
and the right-click cannot disagree. `components::text_menu` (the editing area's right-click)
gains the same three entries above its highlight section, asking the same function.

## 5. The references modal

What the ticket specifies: *"a modal that has the file path, then under that scrolled to the first
reference in that file, with the reference highlight."* That is `Find in Files`' exact anatomy —
results above, a preview of the chosen file below, scrolled to the match, match picked out — and
the modal is built from the same parts: `components::modal` for the frame, dragging and resizing,
`components::splitter` for the divider, the same streamed results, the same
`scroll_to_line(line, pitch, height)` that puts the match a third of the way down the preview with
the spacing-inclusive pitch that function already gets right.

What is different from `Find in Files`, and why:

- **Results are grouped by file.** A file header row — the path relative to the project, and a
  count, `services\text_search.rs · 3` — then that file's references as rows beneath it, each the
  trimmed line with the reference picked out (`picked_out` does this already). Every editor
  surveyed groups this list by file; a flat list makes twenty references in one file read as
  twenty places.
- **Choosing a file row previews that file scrolled to its first reference**, which is the
  ticket's sentence implemented literally; choosing a reference row scrolls to that reference. The
  preview pane draws the file's path at its top, so the answer to "which file am I looking at"
  never depends on the list's scroll position. Arrow keys walk references (skipping header rows),
  `Enter` or a double click opens via `open_the_match`, and the modal closes — the exact contract
  `Find in Files` has.
- **Comment and string references are shown after code references within each file, in the quiet
  colour, suffixed `· comment` or `· string`.** Shown, because a rename that must update a doc
  comment needs to find it; second-class, because they are textual matches and the modal does not
  pretend otherwise. This is the "sure versus maybe" honesty every surveyed tool converges on —
  rope's question marks, IntelliJ's separate text-occurrence group.
- **No query field.** The question was asked by the click; the header says `References to 'name'`,
  and the footer says what the search said: `14 references in 5 files · searching`, then the
  final count, with the cap labelled when it was hit.

The same modal serves multi-definition results (§3.5) with the header `Definitions of 'name'`.
State lives in a struct like `FindInFiles` (the searcher, the hits, the chosen row, the one-shot
`follow` flag whose earned-the-hard-way semantics — do not spend the scroll before the first
result exists — carry over verbatim). Component: `components::references.rs`. Every row names
itself for the tests (`Reference file_marks.rs:41`), and the split is a `settings::Panes` value
clamped like the others.

## 6. Rename

### 6.1 The modal is the preview

`Rename Symbol` opens a modal built from the same furniture: a field pre-filled with the current
name (whole name selected, so typing replaces it), and beneath it the same grouped, previewed
reference list — but with a tick box per row, because **the list is the change set**. The rename
that is applied is exactly the ticked rows, nothing else. IntelliJ reaches its preview through a
dialog and VS Code hides it behind `Shift+Enter`; Unluminate has one modal where the preview *is* the
interface, because on a syntactic tier the user's confirmation is the correctness mechanism, and
a preview that can be skipped is a preview that will be.

Default ticks, decided by what the mechanism actually knows:

| The chosen identifier resolves to | Ticked by default |
|---|---|
| a `variable`-kind definition in this file, or no known definition | code occurrences in **this file** only |
| a function, type, constant or module definition | code occurrences in **every file** |
| — and in every case | comment and string occurrences **unticked** |

A parameter is the first row of that table working as intended: `fn draw(area: Rect)` gives
`area` no definer-keyword definition, so renaming it defaults to this file's code occurrences —
the same scoping instinct behind IntelliJ disabling "text occurrences" for locals — and the
project-wide rows are still there to tick when the same name in another file really is the same
thing. Everything is visible; only the default is scoped.

Two guards, both answered in the modal's footer before anything is applied:

- **The new name must be a word of this grammar** — every character passes
  `is_word_character`, and the whole of it is not one of the grammar's keywords. `RENAME` stays
  disabled with the reason shown: `'match' is a Rust keyword`.
- **A collision is a warning, not a refusal**: if the new name already has a definition in any
  file holding a ticked row, the footer says `'draw' is already defined in layout.rs` in the
  warning colour. The mechanism cannot know whether the collision shadows (that is semantic), so
  it says what it does know and leaves the decision with the person — Roslyn repairs by
  qualification, IntelliJ raises a conflict dialog, and a syntactic tier's honest equivalent is a
  visible warning over a preview.

### 6.2 Applying it

One new document command, `Command::ReplaceMany(Vec<(Range<usize>, String)>)`, applied back to
front so earlier ranges never shift later ones. Undo in Unluminate restores a snapshot, so the whole of
a document's rename is **one undo step** by construction — one `push_undo`, then the edits. The
ranges ride the same shifting `insert`/`remove_range` already do for `chars` and highlights, so
marks and the caret move with the text.

The write itself follows the ownership rule from §3.3:

- **Open files**: `ReplaceMany` on the `Document`. The tab shows the change, `modified` is set,
  undo works per file — and the modal's summary says so: `3 files changed in open tabs — save
  when ready`, because a rename must never silently write a buffer the person was editing.
- **Closed files**: read the file, **verify every ticked range still holds the old name** —
  a file that changed since the search is skipped whole and reported by name, never patched on
  faith — apply the replacements back to front in memory, write the file once. Bytes outside the
  replaced ranges are untouched, so encodings, line endings and trailing whitespace survive
  byte-for-byte. `services::file_marks` shifts that file's stored highlight ranges by the same
  edits, because `FileMarks` owns a closed file's marks and a rename is the one new place a
  closed file's bytes move.

There is no cross-file undo transaction, and that is stated rather than half-built: open files
undo one step each; disk files were changed only after an explicit preview and are re-editable by
the same modal (rename back). VS Code lived for years on exactly this (its cross-file undo prompt
is a mitigation over per-file undo stacks); the preview-first design is what makes it acceptable.

### 6.3 What rename does not touch

The rename never renames files (`Layout` the struct, not `layout.rs` — a file rename is the
explorer's existing rename, and coupling them is semantic work), never edits inside `target` and
friends (the walker already excludes them), and never touches a file with no ticked rows.

## 7. A frame costs what it always cost

The four rules from `task-1666`, applied to what is new:

- **Nothing that runs once a frame allocates.** The hover resolution caches `(text_revision,
  word_range, answer)`; index lookups are hash probes on interned names; the underline is painted
  from the cached range. Moving the pointer with the modifier held does no reading and no `String`
  building.
- **Nothing reads the project where the window draws.** The index builds and rebuilds on its
  worker; references stream from the search thread; the only synchronous file read in the whole
  design is the one-file re-check at the moment of a jump or a closed-file rename, both of which
  are a person's deliberate action, not a frame's.
- **Superseded work stops.** Both workers carry the `AtomicU64` generation; a rebuild or search
  overtaken mid-walk returns without sending.
- **A caret move is not a change to the text.** Everything new is keyed on `text_revision`, never
  `revision`, so selecting and scrolling recompute nothing.

And the measuring instrument comes with it: `crates/unluminate-app/examples/symbol_cost.rs`, the
`frame_cost` pattern — open a real project, build the index, time the build, time a thousand
hover lookups, run a reference search, print each cost. Not a test (a millisecond threshold is a
different number on every machine); the *work counts* are the tests — how many files the index
read, how many definitions one file produced, that a hover lookup performed zero allocations —
because those are the same on every machine.

Budgets, against the numbers gathered in research and measured in the codebase: index build
< 500 ms cold on the search thread (618 files read in 20 ms and tokenised at 1.4 ms / 170 KB
leave room to spare); hover resolve < 0.1 ms; references complete in ~20–30 ms at this project
size and stream on arrival regardless of size; every interactive answer inside Nielsen's 100 ms
"feels like my own action" bound on the project sizes the walker already serves.

## 8. Reachable from the command line

The menu entries come free through `action list`. Beyond them, catalogue rows and `app/cli.rs`
arms, documented in `unluminate-cli/docs/commands.md` under the existing doc-or-fail test:

| Command | What it does |
|---|---|
| `editor definition` | resolve the word at the caret (or `--offset`), print candidates as JSON, `--open` to jump |
| `editor references [name]` | run the search, print `path:line:col` rows plus role; no window needed for reading the answer out of the modal state |
| `editor rename <new-name> [--scope file\|project] [--include comments,strings]` | the modal's default-tick rules as flags; prints the change set; `--apply` applies it |
| `editor navigate-back` / `navigate-forward` | the stack |

The CLI path goes through `UnluminateApp::run_cli` into the same functions the modal uses — the rule
that a thing done from the command line and the same thing done by hand are the same thing — which
is also what lets twenty renames across a project be scripted the way `highlight apply` already
is.

## 9. The scenario battery

The ticket asks that the mechanism be battle tested through ample scenarios. These are the
scenarios, each with its expected behaviour and the test layer that holds it (1 = unluminate-core unit
test, 2 = unluminate-app unit test, 3 = screenshot test, 4 = real window / CLI). "The mechanism works
flawlessly" on a syntactic tier means: **every answer it gives is a true statement about the text,
every guess is visibly a guess, and no input can make it lie, crash, or corrupt a file.**

### 9.1 Resolving and jumping

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 1 | `fn draw` in the same file, click on a `draw(` call | one candidate, jump, name selected | 1, 3 |
| 2 | definition in another file | jump opens the tab, selects, explorer follows (existing `follow_the_open_file`) | 2, 4 |
| 3 | two files each define `new` | modal lists both, ranked §3.5; no silent jump | 1, 3 |
| 4 | shadowing: `let x` at lines 3 and 10, click `x` at line 12 | nearest-above ranks first (line 10) | 1 |
| 5 | click on a keyword, number, operator, or inside a comment/string | no underline, no action, ordinary caret click | 1 |
| 6 | click on a word with no definition anywhere | menu path: status bar `No definition found for 'x'`; click path: no underline, caret placed | 2, 3 |
| 7 | modifier-hover over a resolvable word | underline + pointer cursor; released modifier removes both | 3 |
| 8 | already on the definition | pivots to references modal | 2, 3 |
| 9 | TS class method `render(area) {` | `Likely` definition found via brace heuristic | 1 |
| 10 | `if (ready) {`, `list.map(x => {`, `draw(area)` call | none of them is a definition | 1 |
| 11 | definition moved on disk since the index read it | re-check at click re-finds it; jump lands on the name | 2 |
| 12 | file deleted since indexing | jump reports the open error in the status bar (existing open path), nothing crashes | 2 |
| 13 | open tab with unsaved edits owns its definitions | click resolves against live text, not the disk | 2 |
| 14 | unicode identifiers (`déjà`, CJK, emoji in a string nearby) | ranges land on char boundaries; no panic; selection correct | 1 |
| 15 | identifier at byte 0, and at the last byte of the file | `identifier_at` returns the full word at both edges | 1 |
| 16 | CSS `--brand-hue` | go to definition absent for the file; no menu entry | 2 |
| 17 | `.txt` / `.md` / picture tabs | all three entries absent; modifier-click is an ordinary click | 2, 3 |
| 18 | navigate back after two jumps, then forward | returns through both offsets, reopening closed tabs; new jump clears forward | 2 |
| 19 | same source, index built twice | identical index — determinism, the property every screenshot rests on | 1 |

### 9.2 References

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 20 | name used in 5 files | grouped by file with counts; first file's first reference previewed, highlighted, scrolled a third down | 3 |
| 21 | choose a file header row | preview scrolls to that file's **first** reference (the ticket's sentence) | 2, 3 |
| 22 | choose a later reference row | preview scrolls to that reference | 2 |
| 23 | `count` vs `counter`, `x` vs `x2` | whole-word only; no partial matches | 1 |
| 24 | occurrences in comments and strings | listed after code per file, quiet colour, `· comment` / `· string` suffix | 1, 3 |
| 25 | word occurs 10,000 times (pathological) | capped at `LIMIT`, footer says `the first 500 · there are more`, window never blocks | 2 |
| 26 | invoked with caret on whitespace / no identifier | status bar message, modal does not open | 2 |
| 27 | reference in a file with unsaved open edits | live text searched for that file, disk for the rest | 2 |
| 28 | search overtaken (modal closed, reopened on another word) | old generation abandoned mid-walk; no stale rows | 1 (the `text_search` test pattern) |
| 29 | minified one-line file | line trimmed by the existing `shorten`; match still inside it | 1 |
| 30 | `Enter` / double-click on a row | opens via `open_the_match`, selection on the reference, modal closed | 3 |
| 31 | generated folders (`target`, `node_modules`) | never searched (existing walker guarantee), stated in docs | 2 |
| 32 | project of 100k files | streaming rows appear while walking; cancellation still instant; no memory spike (no stored occurrence index) | 4 |

### 9.3 Rename

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 33 | rename a function used in 4 files, all closed | preview all ticked; apply rewrites each file once; bytes outside ranges identical; re-running references finds only the new name | 1, 2 |
| 34 | rename a parameter | default ticks scoped to this file's code; other-file rows present, unticked | 1, 2 |
| 35 | rename a `let` local | same file-scoped default | 1 |
| 36 | rename a class/type | project-wide default | 1 |
| 37 | untick two rows, apply | exactly the ticked rows change — the unticked lines byte-identical | 1 |
| 38 | new name is a keyword / has a space / empty | `RENAME` disabled, reason in footer | 1, 3 |
| 39 | new name already defined in a ticked file | warning line, rename still allowed | 1, 3 |
| 40 | rename in an open modified tab | `ReplaceMany` on the document; **one undo step**; `modified` set; not written to disk | 1 |
| 41 | undo after 40 | the document's text and its highlight marks restore exactly (snapshot undo) | 1 |
| 42 | closed file changed on disk between search and apply | that file skipped whole, reported by name; other files still applied | 1 |
| 43 | closed file with stored highlight marks overlapping the rename | `FileMarks` ranges shifted by the edit deltas; reopening shows marks on the same words | 1, 2 |
| 44 | occurrences in strings/comments ticked by hand | applied like any ticked row | 1 |
| 45 | rename where old name occurs twice on one line | both replaced; back-to-front application keeps ranges true | 1 |
| 46 | new name longer / shorter / same length as old | all three shift arithmetics correct (the classic off-by-one family) | 1 |
| 47 | rename with zero ticked rows | `RENAME` disabled | 2 |
| 48 | CRLF file, closed | line endings preserved byte-for-byte outside the ranges | 1 |
| 49 | rename via CLI `--apply` with the window open on one affected file | open tab edited as a document, closed files on disk — same split as the modal | 4 |
| 50 | two panes showing two files both affected | both tabs update (each pane's document edited once); no double edit | 2 |

### 9.4 The properties that hold everywhere

Four invariants, one test each, in the spirit of `mermaid::check::properties`:

- **Truthfulness**: every range any function returns lies inside the text it was derived from, on
  character boundaries, and the text at a definition's range is the definition's name.
- **Determinism**: same text, same grammar → identical definitions, occurrences and ranking, every
  time.
- **Non-destruction**: for every rename applied to a buffer or file, the result equals the input
  with exactly the ticked ranges substituted — verified by reconstructing it independently in the
  test.
- **Isolation**: no function in `unluminate_core::symbols` performs I/O; the app-side workers touch
  only files under the project root from `all_files`.

## 10. What is built where

| Piece | Crate / place |
|---|---|
| `symbols` module: definitions, `identifier_at`, occurrences, ranking, `ReplaceMany` | `unluminate-core` (unit tests, no window) |
| `Grammar::definers`, `brace_definitions` + manifest parsing | `unluminate-core` / `services::plugins` |
| `symbol_index` worker, references search mode | `unluminate-app/services` |
| references + rename modal | `unluminate-app/components/references.rs` |
| underline, `JumpRequest`, landing | `components/editor_view.rs`, `app/mod.rs` |
| actions, menus, text menu, `file_kind` gate | `app/actions.rs`, `components/text_menu.rs`, `services/file_kind.rs` |
| navigation stack | `app/mod.rs` |
| CLI rows + arms + docs | `unluminate-cli/src/catalogue.rs`, `app/cli.rs`, `unluminate-cli/docs/commands.md` |
| `symbol_cost` example | `unluminate-app/examples` |

## 11. Sources

The load-bearing research, so a later reader can check a claim rather than trust it:
LSP 3.17 specification (microsoft.github.io/language-server-protocol) — protocol surface, UTF-16
positions, `WorkspaceEdit`; M. Peyton Jones, *LSP: the good, the bad, and the ugly* — the
version/causality gap; rust-analyzer blog, *Find Usages* — text-search-then-verify architecture
and search scopes; go.dev/blog/gopls-scalability and rust-analyzer/tsserver issue trackers — the
memory and stall numbers; Sublime Text indexing docs and GitHub's code-navigation docs — the
syntactic tier as a shipped product (24 languages, <100k-file repos); microsoft/vscode-anycode —
the same tier, by the LSP's own authors; github/stack-graphs — archived September 2025; VS Code
docs (editingevolved, refactoring, theme-color) — peek/panel anatomy, `gotoLocation.multipleDefinitions`,
refactor preview, landing highlight cleared on caret move; JetBrains help (Find Usages, Rename
refactorings, Rename dialogs) — Shift+F6/Alt+F7, in-place-then-dialog escalation, comment/string
checkboxes, base-method prompt; rope docs — the question-mark treatment of unsure matches;
Roslyn rename design — conflict detection and repair; universal-ctags docs and Russ Cox,
*Regular Expression Matching with a Trigram Index* — index formats and the brute-force/index
crossover math; Nielsen Norman Group, *Response Times: 3 Important Limits* — the 100 ms bound.

## 12. What is deliberately not here

- **No language server client** — weighed in §2.1; the two-question seam in `symbols` is where one
  would land if the product ever wants the semantic tier, and nothing above the seam would change.
- **No tree-sitter, no stack graphs** — §2.2.
- **No semantic resolution**: no scopes, no types, no import following. The mechanism never claims
  it; ranking plus the candidate modal is the honest substitute.
- **No change-signature, extract-function, or file-rename refactorings** — renaming identifiers is
  the ask.
- **No stored occurrence index** — §3.4 says why the search is the better answer at this scale,
  and the trigram-index crossover (worth revisiting around the 50–100× project-size mark) is
  recorded there.
- **No `documentHighlight`-style occurrence marking under a resting caret** — a natural follow-up
  (`occurrences` already answers it); left out to keep this ticket's surface reviewable.
