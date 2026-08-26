# task-1677 — auto-complete: dropdown suggestions while typing

## 1. What was asked

> Task 1676 recently did some code support.
>
> We want to implement auto-complete, so as I'm typing, I see dropdown suggestions I can arrow and
> tab to complete, similar to IntelliJ.

One feature, and it is the reading half of what `task-1675`/`task-1676` built: those tickets taught
Quill to answer "where is this name defined and used" after the name exists; this one offers the
name while it is still being typed. The same machinery answers both — the tokeniser's reading of
the file, the per-tab `FileSymbols`, the project's definition index — which is what makes this
ticket small where most editors' completion is enormous: **every source of suggestions this design
needs is already in memory, kept fresh by keys that already exist.** What is new is a scoring
function, a dropdown, and the key routing that lets a list be steered while typing carries on
underneath it.

"Similar to IntelliJ" is read as IntelliJ's *behaviour*, not its knowledge: the popup appears as
you type without being asked, typing narrows it, `Up`/`Down` walk it, `Tab` and `Enter` take the
chosen row, `Escape` puts it away, and none of it ever blocks a keystroke. IntelliJ's *knowledge* —
type-aware member lists after a dot — is semantic, and `task-1675` §2 already weighed and rejected
the mechanisms that provide it (a language server, tree-sitter). This ticket builds the same tier
those features stand on: the identifier tier, honest about being syntactic.

## 2. What the surveyed editors do

The research looked at how each mechanism-tier of editor completes, because the behaviour worth
copying and the mistakes worth avoiding are both documented.

### 2.1 IntelliJ — the behaviour the ticket names

The popup opens automatically as you type ("Show suggestions as you type", on by default) and can
be summoned with `Ctrl+Space`. Typing any part of a word narrows the list — middle matching, not
prefix-only — and matching is case-insensitive unless asked otherwise. Suggestions sort by
relevance, with alphabetical as an option. The two acceptance keys mean different things:
**`Enter` inserts the chosen suggestion to the left of the caret; `Tab` replaces the characters to
the right of the caret as well** — accept versus accept-and-replace, and both are kept in this
design because the distinction is exactly right when the caret is in the middle of an identifier
being retyped. `Escape` dismisses. A second invocation widens the search — a tiering this design
does not need, because its one tier answers instantly.

### 2.2 VS Code — the word tier as a shipped product, and the `Enter` lesson

VS Code's IntelliSense is three sources layered: language-server suggestions, snippets, and
**word-based suggestions** — plain words harvested from the current and matching open documents,
serving every file type with no language service at all (`editor.wordBasedSuggestions`, default
`matchingDocuments`). That is this design's tier, shipped by the most-used editor there is.

The lesson worth importing is `editor.acceptSuggestionOnEnter`: enough people pressed `Enter`
meaning "new line" and got a suggestion instead that VS Code grew a three-way setting (`on` /
`smart` / `off`, where `smart` accepts only when the acceptance would actually change the text).
§5.4 says how this design blunts the same trap without a setting: the row equal to what was
already typed is never offered, so `Enter` on a completely-typed word has nothing to swallow.

### 2.3 Sublime Text — the scoring that makes fuzzy feel right

Sublime's completion index is built by background scanning, and its match is a subsequence — `stdr`
finds `stderr` — scored by heuristics that have been reverse-engineered and reused across the
industry (they are also what `fzf` and VS Code's own filtering converge on): a bonus for a match
after a separator or a camelCase boundary (worth the most), a bonus for consecutive matched
letters, a penalty per unmatched letter (so shorter candidates win), a small penalty for skipped
leading letters. §4.3's scoring is this rubric, applied to identifiers instead of file names —
and it is the second time Quill has chosen the shape: `services::file_search` already ranks file
names by subsequence with a boundary preference, for `Go to File`.

### 2.4 Helix — a word index kept on a thread, and what a trigger length is for

Helix grew word completion in 2025 (PR #13206): words from all open buffers into a database,
updated off the main thread by examining only the changed windows of text, debounced, fuzzy-matched
case-insensitively, with a configurable `word-completion-trigger-length` (users argued it down from
7 toward 2–3). The design below does not need the thread or the debounce — §4.1 says why Quill's
sources are already maintained — but the trigger-length discussion settled §5.1's default of 2.

### 2.5 Vim — the floor

`Ctrl+N`/`Ctrl+P` complete the word from the buffers listed in `'complete'` — manual, no popup by
default, prefix-driven. It is the proof that even the plainest identifier completion is a feature
people build whole workflows on; it is also the floor this design should comfortably clear: Vim
asks the person to remember to ask, and the ticket asks for suggestions that arrive unasked.

### 2.6 What was rejected

- **A language server client, tree-sitter, semantic member completion** — rejected in
  `task-1675` §2 for reasons that have not changed; completion inherits the verdict. The one
  architectural courtesy carries over too: everything the popup shows comes through one function
  (§4.2), and a semantic tier, if the product ever wants one, would feed the same popup through the
  same seam.
- **Snippets / templates** (`for` expanding to a loop skeleton) — a different feature with a
  different data model (placeholders, tab stops), and nothing in the ticket asks for it.
- **Ghost text** (a single grey inline suggestion, Copilot-shaped) — a different interaction
  model; the ticket asks for a dropdown.
- **An occurrence database of other files' words** (Helix's `WordDB`, VS Code's `allDocuments`) —
  rejected as the mirror of `task-1675` §3.4: Quill already indexes the cross-file names worth
  offering — the **definitions** — and they are better candidates than raw words, because they
  carry a kind and a defining file. Harvesting every word of every file would add an index that
  needs invalidation, to offer lower-quality rows. The current file's own words are included
  (§4.1), and they cost nothing: `FileSymbols` already holds them.

## 3. The shape of the design

Three new pieces, each in the layer Quill already puts that kind of thing:

| Piece | What it is | Where |
|---|---|---|
| `quill_core::completion` | the pure half: the stem under a caret, the match, the score, the ordering | `quill-core`, tests with no window |
| `QuillApp` completion state | one open popup at most, its candidate rows, which is chosen, keyed on `text_revision` | `app/completion.rs`, beside `app/symbols.rs` |
| `components::completion` | the dropdown: draws rows, reports what the keys and clicks meant, decides nothing | `quill-app/components/completion.rs` |

And no new threads, no new index, no new watcher. The sources are the per-tab `FileSymbols`
(already kept fresh, keyed on `Document::text_revision()`), the open tabs' `TabSymbols::named`
lists, `services::symbol_index` (already built on its worker, already generation-cancelled), and
the file's `Grammar` (already in memory). Completion *reads* what `task-1676` already maintains.

## 4. The candidates

### 4.1 Three sources, one ownership rule

For a stem typed in a file whose language has a grammar, the candidate pool is:

1. **The words of this file.** `FileSymbols` already collects every identifier-shaped token into
   its sorted `words` list. One addition: a **distinct word list** derived from it — each unique
   spelling once — computed in the same pass or from the ranges afterwards, cached on
   `TabSymbols` beside `named`, keyed on the same `text_revision`. This is what completes locals,
   parameters, field names and anything else the definers cannot see, and it is what makes
   completion work in a CSS file, where `task-1675` deliberately defined no definers.
2. **Definitions, everywhere.** The open tabs' `TabSymbols::named` lists first, then
   `symbol_index::Index` for every closed file — with the open files' paths dropped from what the
   index says, which is the ownership rule of `task-1675` §3.3 verbatim: *a file that is open is
   owned by its `Document`, and every other file is owned by the index.* A definition candidate
   carries its `SymbolKind` and its defining file, which is what lets the row say
   `draw_frame · layout.rs` where a bare word can only say itself.
3. **The language's own words.** The grammar's `keywords`, `builtins` and `types` lists — `fn`,
   `match`, `usize`, `Some` in Rust; `display`, `flex` in CSS. They are in the manifest already;
   completion is the second reader of the same data.

The pool is deduplicated **by exact spelling** — what is inserted is the name, so two entries that
would insert the same bytes are one row. The best source wins the row's label: a definition beats
a keyword beats a plain word, because a definition has the most to say about itself.

One row is always removed: **the candidate equal to the stem itself** (§5.4 says what this buys).

The `symbol_index::Index` needs one addition to serve this: today it answers only
`definitions_of(&str)`, an exact-name hash lookup. It gains a sorted list of its distinct names
(it already counts them — `names()` — so the list exists in spirit), so that a prefix walks a
binary-searched range and a subsequence scan walks the whole list once. At the measured scale —
4,445 names on Quill's own repository — a full scan per keystroke is nothing (§7).

### 4.2 One function answers, and the popup only draws

Everything above is gathered by one method on the window —
`QuillApp::completion_candidates(stem, …) -> Vec<Row>` — called when the stem changes and **never
per frame**. The popup component receives rows and draws them; it does not know what an index is.
This is the `candidates_for` pattern from `app/symbols.rs`, and it is the seam a semantic tier
would land behind if one ever arrives.

### 4.3 The match and the score

In `quill_core::completion`, pure over `(stem, candidate)`, no I/O, testable with no window:

- **A candidate matches when the stem is a case-insensitive subsequence of it.** `lyt` matches
  `layout`; `psttx` matches `paint_text`; middle matching comes free (`draw` matches `redraw`),
  which is IntelliJ's documented behaviour and Sublime's.
- **The score is the Sublime rubric**, the same one `services::file_search` already applies to
  file names, restated for identifiers:
  - a large bonus when the candidate **starts with the stem** — the commonest intent by far;
  - a bonus per matched letter that sits on a **word boundary** — the start, after `_` or `-`, or
    a lower→upper camel step — so `pt` prefers `paint_text` over `pointer`;
  - a bonus per **consecutive** matched letter beyond a run's first;
  - a small bonus per matched letter whose **case agrees exactly**;
  - a penalty per unmatched candidate letter, so the shorter of two otherwise-equal names wins.
- **Ties are broken so the order is total and deterministic**: by source (this file's definitions,
  then this file's words, then open tabs' definitions, then the index's, then the grammar's), then
  shorter name first, then byte order of the name. Same text, same stem → the same list in the
  same order, every time — the property every screenshot test rests on, and scenario 33 pins it.

The alignment behind the score is found the way Sublime finds it — best alignment rather than
first alignment, by bounded recursion — but **the tests pin orderings, not raw numbers**: a score
is meaningless outside a comparison, and a test asserting `-13` would be a test of the constants
rather than of the behaviour anyone can see.

## 5. The popup

### 5.1 When it opens

- **Automatically, while typing.** After a frame in which the document inserted text, if: the
  file's language has a grammar (§8.1's gate), the setting says automatic (§8.3), the caret sits
  in `Role::Code` (asked of `FileSymbols::role_at` — a doc comment's prose does not want a list
  flickering over it), and the **stem** — the identifier characters immediately left of the caret,
  read by the same `is_word_character` rules everything else uses — is **two or more characters**.
  One character opens on nearly every letter of a file and matches most of it, which is noise;
  Helix's users argued the same question down to "small, but not one". Two is the default, not a
  tunable: the manual key covers the rest.
- **On demand: `Ctrl+Space`, menu entry `Complete Word`** on the Edit menu (§8.2), which works
  from one character, and works inside comments and strings — where the *automatic* popup never
  opens, but a person who asks in a doc comment deserves the file's words. With no identifier
  character to the left of the caret at all, it does what every honest miss in Quill does: the
  status bar says `There is nothing to complete here.`, and no popup opens.

### 5.2 What it looks like

A dropdown anchored under the caret — `Layout::caret_at(head)` plus the pane's text origin, the
same arithmetic the caret itself is painted with — flipped to sit **above** the caret's line when
the rows would cross the bottom of the pane, and clamped inside the pane horizontally. It is drawn
from `components::completion`, built from what `design/style-guide.md` already provides: the
popup frame, menu-height rows (24 points), the one selection pill, the quiet colour for the
secondary text. Up to eight rows are shown; more scroll, and the pill drags the scroll with it.

Each row: the candidate name with the **matched letters picked out in the accent colour** (the
list answers "why is this row here" the way `Find in Files` answers it with `picked_out`); a small
drawn glyph for the kind where the candidate has one (function, type, constant, variable, module —
drawn, not lettered, per the style guide); and a quiet suffix saying where it came from when that
is worth a word — the defining file's name for a definition (`draw_frame · layout.rs`), `keyword`
for the grammar's own words, nothing for this file's words, which need no explanation.

Every row names itself for the tests — `Completion draw_frame` — because a control with no name
cannot be tested at all.

**It is not an `egui::Popup`.** egui keeps at most one popup open at a time — the rule that
already shaped the flyouts and the text menu — and this dropdown must coexist with nothing *and*
must never take keyboard focus: the document keeps the keyboard, typing flows into the file, and
the dropdown is a picture of an offer. It is an `egui::Area` on the foreground order, positioned
by the window each frame from the caret's own geometry, exactly as cheap to draw as the menu it
resembles. It never opens over a modal: a modal already owns the keyboard
(`text_box_has_the_keyboard`), and the editing area stands aside while one is up, so the trigger
in §5.1 never fires there.

### 5.3 The keys while it is open

The routing reuses the machinery the search modals proved: **`consume_key` removes the event from
the frame's input**, so a key the popup takes never reaches `editor_view::handle_input` — the same
one-frame ordering `Find in Files` and `Go to File` already rely on, applied before the editing
area reads its events. Everything not consumed flows through untouched, which is the property that
makes the popup unobtrusive: it takes exactly five keys, and only while it is open.

| Key | While the popup is open |
|---|---|
| `Down` / `Up` | move the pill; clamped at the ends, scrolling the list when it must |
| `Tab` | accept the chosen row, **replacing the whole identifier** under the caret |
| `Enter` | accept the chosen row, replacing the **stem** (word start to caret) only |
| `Escape` | close the popup, nothing else — consumed, so it cannot also clear a selection |
| anything else | not consumed: letters keep typing and refiltering, `Left`/`Right`/`Home`/`End`/clicks move the caret and the popup closes (§5.5) |

`Tab` versus `Enter` is IntelliJ's own distinction (§2.1), and both are kept because both are
right: `Enter` when finishing a fresh word, `Tab` when retyping the front of an existing one —
`dra│w_frame` completed to `draw_the_frame` should not leave `w_frame` dangling behind the caret.
The first row is pre-chosen, so `Tab` alone takes the best match, which is the gesture the ticket
names. A click on a row accepts it the same way `Enter` does; the click never reaches the editing
area, because the popup's `Area` is in front of it and takes the hit.

The existing `Tab` guard in `handle_input` (control held means `Next Tab`) is untouched: the
popup consumes a **bare** `Tab` only, and only while open, so the three meanings of the key — move
tab, indent, complete — stay on their three distinct chords.

### 5.4 What accepting does

One command, already built: `Command::ReplaceMany(vec![(range, name)])` — the range being the stem
for `Enter`, the whole word for `Tab`. One undo step by construction (undo restores a snapshot),
the caret lands at the end of the inserted name, highlight marks and the selection shift exactly
as every other edit shifts them, and `modified` is set because this *is* an edit. Undo restores
the stem as typed — scenario 24 pins it.

And the `Enter` trap from §2.2 is answered structurally rather than by a setting: **the row equal
to the stem is never in the list** (§4.1), so once a word is completely typed the popup has either
something genuinely longer to offer or nothing at all — and with nothing to offer it has already
closed (§5.5), so `Enter` means what the person meant: a new line. VS Code's `smart` mode
approximates this at acceptance time; dropping the no-op row does it at candidate time, which also
stops the list offering a row that would do nothing.

### 5.5 When it closes

The popup closes when any of these happens, and the rule underneath them is one sentence: **it is
open only while it is an answer to the word being typed at the caret.**

- the stem no longer matches anything (typing narrowed the list to zero — it does not linger empty);
- the stem is gone (the word boundary was typed, backspace erased it, the word was accepted);
- the caret moved by anything other than typing — click, `Left`/`Right`/`Home`/`End`, a jump, a
  scroll command; a wheel scroll that leaves the caret on screen keeps it (the anchor moves with
  the text, recomputed per frame from `caret_at`), one that takes the caret's line off screen
  closes it;
- `Escape`, accepting a row, the pane losing the keyboard, the tab changing, a modal opening;
- an automatic-popup file becoming one where completion is absent (the plugin was switched off).

Closing is nothing but dropping the state: no animation, no memory, nothing written anywhere.

## 6. Where the state lives

One `Option<CompletionState>` on `QuillApp` — not per tab, because at most one popup exists and it
belongs to the pane with the keyboard, the same reasoning as the one `hover` and the one
`references` modal. The state: the stem's range, the rows, which is chosen, the scroll, and the
`text_revision` the rows were computed at. **Rows are recomputed when and only when the revision
or the stem range moved** — a caret blink, a repaint, a frame of idling recomputes nothing, which
is `task-1666`'s rule (nothing that runs once a frame may allocate) kept the same way `hover`
keeps it: compare two integers, and only a change does work.

The pane loop borrows the focus (`task-1664`); the popup is drawn **after** the pane loop by the
window, from the focused pane's recorded geometry, so it is never underneath a divider or a later
pane and never draws twice in a split view. Key routing runs before the focused pane's
`handle_input`; drawing runs after everything; both read the same state.

## 7. What it costs

Measured inputs, from `symbol_cost` on Quill's own repository: 4,445 distinct names in the index,
11,497 words in the largest file (a 234 KB test file — distinct spellings are far fewer), a
`FileSymbols` read at 1.8 ms per text revision on that file, grammar lists in the tens.

- **Per keystroke while the popup is open or opening**: gather + dedup + score + sort over roughly
  ten thousand candidates. A subsequence score is a short walk over two small strings;
  generously, this is single-digit milliseconds in release on the biggest file in the repository,
  and the common case — a source file a tenth that size — is well under one. It runs at keystroke
  time on the window's thread, deliberately: the sources are already in memory, so there is
  nothing to wait for, and a worker thread here would add generation plumbing to make an instant
  answer arrive a frame late. This is the one budget the implementation must measure and honour:
  **under 5 ms on the largest file in this repository**, and if a future project breaks it the
  answer is capping the pool (an honest `LIMIT`, the references modal's pattern), not a thread.
- **Per frame while open, nothing changing**: two integer comparisons and drawing at most eight
  rows from cached strings. No allocation, no scoring, no reads.
- **Per text revision**: the distinct-word list, one pass over `FileSymbols::words` already being
  rebuilt at that moment anyway, into a sorted `Vec<String>` cached on the tab.
- **The measuring instrument**: `crates/quill-app/examples/completion_cost.rs`, the
  `frame_cost`/`symbol_cost` pattern — open a real project, type a stem against the largest file,
  print gather/score/sort costs and the candidate counts. The *counts* are the tests; the
  milliseconds are the report.

## 8. Fitting into the window

### 8.1 The gate

`services::file_kind` gains `completion_applies(path, grammars)`: **true when a switched-on
language plugin claims the file** — nothing more. CSS completes (its words and its keywords are
real, even with no definers, which is why source 1 and source 3 exist), Mermaid completes its
keywords, and Markdown, plain text and pictures have no popup and no menu entry — absent, not
dimmed, the rule every inapplicable control follows. The menu, the automatic trigger and the CLI
all ask this one function, so they cannot disagree.

### 8.2 The action

One new `Action::CompleteWord` — menu entry `Complete Word` on the Edit menu beside the symbol
entries, key `Ctrl+Space` on both platforms (IntelliJ's binding; on macOS the system may have
claimed it for input sources, and the menu entry works regardless — the note belongs in the menu
test, not in a different binding). Absent when `completion_applies` says no. Through
`quill-cli action list` it is scriptable the day it exists.

### 8.3 The setting

One settings key, `editor.suggestions`, values `automatic` (default) and `manual`, surfaced as a
tick box in the Settings window's editor section: *Suggest completions as you type*. `manual`
keeps `Ctrl+Space` and everything else and only stops the unasked popup — the person it exists
for is the one who found the flicker noisy, and for them a third value would be the off switch
that `manual` already is. Settings precedent: `Settings::shell()`, one function that answers.

### 8.4 The command line

Two catalogue rows, arms in `app/cli.rs`, sections in `quill-cli/docs/commands.md` under the
doc-or-fail test:

| Command | What it does |
|---|---|
| `editor complete [--offset N] [--limit N]` | print the candidate rows for the caret (or offset) as JSON — name, kind, source, score order — without opening the popup |
| `editor complete --choose <name>` | apply the named candidate to the stem at the caret, exactly as `Enter` would |

Both go through `QuillApp::run_cli` into the same functions the popup uses, so a thing done from
the command line and the same thing done by hand are the same thing — and the agent assessment
(`quill-cli/agent-assessment`) gets a completion surface it can drive and grade.

## 9. The scenario battery

Layer numbers as in `task-1675` §9: 1 = quill-core unit test, 2 = quill-app unit test,
3 = screenshot test, 4 = real window / CLI.

### 9.1 Matching and ranking

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 1 | stem `dra`, file defines `draw`, `draw_frame`, `redraw` | all three match; `draw` first (prefix + shortest), `redraw` last (no prefix) | 1 |
| 2 | stem `pt` against `paint_text` and `pointer` | boundary bonus ranks `paint_text` first | 1 |
| 3 | stem `LYT` against `layout` | case-insensitive match; exact-case bonus never *excludes* | 1 |
| 4 | stem equal to a candidate (`draw` typed, `draw` in file) | that row is absent; `draw_frame` still offered | 1 |
| 5 | stem matching nothing | empty answer | 1 |
| 6 | two sources offer one spelling (`let` the keyword, `let` a word in a string) | one row, labelled from the better source | 1 |
| 7 | definitions outrank same-scored plain words; keywords come after both | pinned ordering | 1 |
| 8 | unicode identifiers (`déjà`, CJK) as stem and candidate | char-boundary-safe, no panic, correct match | 1 |
| 9 | CSS: stem `--br` against `--brand-hue` | the hyphens are word characters here; matches, boundary bonus lands | 1 |
| 10 | same text, same stem, scored twice | identical order — determinism | 1 |
| 11 | empty stem / one-char stem via the automatic path | no candidates asked for at all | 1, 2 |

### 9.2 Opening and closing

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 12 | typing the 2nd word character in a Rust file | popup opens, first row pre-chosen | 2, 3 |
| 13 | typing in a Markdown file / plain text / a picture tab | nothing, ever; menu entry absent | 2 |
| 14 | typing inside a comment or a string | no automatic popup; `Ctrl+Space` there opens one | 2 |
| 15 | typing narrows to zero matches | popup closes; typing on to a match does not reopen it unasked until the next insertion | 2 |
| 16 | backspace within the word, stem still ≥ 1, popup open | stays open, refiltered | 2 |
| 17 | word boundary typed (space, `(`, `.`) | closes | 2 |
| 18 | click elsewhere / `Left` / `Home` / jump / tab switch / modal opens | closes; the key or click still does its ordinary work | 2 |
| 19 | `Escape` | closes; consumed — a selection in the document survives | 2 |
| 20 | `editor.suggestions = manual` | no automatic popup; `Ctrl+Space` works | 2 |
| 21 | `Ctrl+Space` with no identifier left of the caret | status bar message, no popup | 2 |
| 22 | popup open near the bottom of the pane | drawn above the caret's line, on screen | 3 |
| 23 | split view, both panes on source files | one popup at most, in the pane with the keyboard | 2, 3 |

### 9.3 Steering and accepting

| # | Scenario | Expected | Layer |
|---|---|---|---|
| 24 | `Down` twice, `Enter`; then undo | third row's name replaces the stem, caret after it, one undo step restores the stem | 2 |
| 25 | `Down`/`Up` at the ends | clamped, no wrap; list scrolls when the pill walks off the eight | 2, 3 |
| 26 | `Tab` with the caret mid-word (`dra│wing`) | whole word replaced by the chosen row | 1, 2 |
| 27 | `Enter` with the caret mid-word | stem replaced, `wing` untouched to the right | 1, 2 |
| 28 | arrows with the popup open | caret's line does not move — the keys were consumed | 2 |
| 29 | typing while open | letters land in the document *and* refilter the list, in order | 2 |
| 30 | click on a row | accepts as `Enter`; the click never places the caret behind the popup | 2, 3 |
| 31 | accept a definition candidate from a closed file | the text is inserted; nothing is opened, nothing read from disk | 2 |
| 32 | `Ctrl+Tab` while open | tab switching untouched; the popup closes with the tab change | 2 |
| 33 | popup rendering: rows, pill, matched-letter accent, kind glyphs, quiet suffixes | the screenshot baseline | 3 |
| 34 | highlight marks after the replaced range | shifted, still on their words | 1 |
| 35 | CLI `editor complete` against a real window; `--choose` then read the document | list matches the popup's; the edit landed; documented examples run | 4 |

### 9.4 The properties that hold everywhere

Four invariants, one test each, the `mermaid::check::properties` spirit:

- **Truthfulness**: accepting a row changes exactly the stem (or word, for `Tab`) and inserts
  exactly the row's name — reconstructed independently in the test; every range handled lies on
  char boundaries inside the text it came from.
- **Determinism**: same text, same grammar, same stem → the same rows in the same order.
- **Isolation**: nothing in `quill_core::completion` performs I/O; the window's gathering touches
  only what is already in memory (documents, `TabSymbols`, the index, grammars) — a keystroke
  never reads a disk.
- **Non-interference**: with the popup closed, every key means what it meant before this ticket;
  with it open, exactly the five keys of §5.3 are consumed and nothing else changes meaning.

## 10. What is built where

| Piece | Crate / place |
|---|---|
| `completion` module: stem, match, score, order, dedup | `quill-core/src/completion.rs` (unit tests, no window) |
| distinct word list on `FileSymbols` / cached on `TabSymbols` | `quill-core/src/symbols.rs`, `app/symbols.rs` |
| sorted name list on the index | `services/symbol_index.rs` |
| `CompletionState`, gathering, trigger, key routing, accept | `app/completion.rs` (new, beside `app/symbols.rs`) |
| the dropdown | `components/completion.rs` |
| `completion_applies` gate | `services/file_kind.rs` |
| `Action::CompleteWord`, menu entry, key | `app/actions.rs` |
| `editor.suggestions` setting + tick box | `services/store.rs` settings, `components/settings_dialog.rs` |
| CLI rows + arms + docs | `quill-cli/src/catalogue.rs`, `app/cli.rs`, `quill-cli/docs/commands.md` |
| `completion_cost` example | `crates/quill-app/examples/completion_cost.rs` |

Build order that keeps every layer green as it goes: the core module and its tests; the word
list and index addition; the state and gathering with unit tests; the component and its
screenshots; the trigger and key routing; the action, gate, setting; the CLI and docs; the
measuring example; then the battery swept end to end and the real window exercised (layer 4:
type, steer, accept, and drive `editor complete` against the live window).

## 11. Sources

JetBrains, *Code completion* (jetbrains.com/help/idea/auto-completing-code.html) — auto-popup
default, middle matching, case handling, relevance sort, and the `Enter`-inserts /
`Tab`-replaces distinction; JetBrains support threads on `Choose Lookup Item Replace` — the same,
as users meet it; VS Code docs, *IntelliSense* (code.visualstudio.com/docs/editing/intellisense) —
word-based suggestions as the no-language-service tier, `editor.wordBasedSuggestions`,
`editor.acceptSuggestionOnEnter` (`smart`) and why it exists; F. Reda, *Reverse Engineering
Sublime Text's Fuzzy Match* (forrestthewoods.com) — the subsequence-with-bonuses rubric and its
measured cost; Sublime Text docs, *Completions* — the background index and Tab-walk; Helix PR
#13206 (github.com/helix-editor/helix/pull/13206) — cross-buffer word DB off-thread, trigger
length discussion, debouncing; Vim documentation, *insert.txt* — `Ctrl+N` and the `'complete'`
option; Nielsen Norman Group, *Response Times* — the 100 ms bound this design's keystroke budget
sits far inside; `task-1675-code-editing-tdd.md` and the `task-1676` implementation — the sources,
the ownership rule, the seam, and the measured numbers reused throughout.

## 12. What is deliberately not here

- **No semantic completion** — no members after a dot, no types, no imports. The rejection is
  `task-1675` §2's, inherited; the §4.2 seam is where a semantic tier would land.
- **No snippets or templates** — a different feature; nothing in the ticket asks for it.
- **No ghost text / inline AI suggestions** — a different interaction model.
- **No cross-file word harvesting** — §2.6; the index's definitions are the cross-file offer.
- **No frequency or recency ranking, no ML** — both trade determinism (the property the
  screenshot tests and the CLI's stable output rest on) for a marginal reorder of a short list.
  If the plain rubric ever feels wrong in the fingers, that is the follow-up to file, with
  examples.
- **No documentation panel beside the popup** — there is no documentation to show at this tier;
  a definition row already names its file.
- **No auto-insert on a unique match, no completion on `.` or `::`** — both are semantic bets a
  syntactic tier loses often enough to annoy.
- **No new threads and no debounce** — the sources are maintained already; §7 says why the
  keystroke path can afford to be synchronous, and what to do (a cap, not a thread) if a future
  scale breaks the budget.
