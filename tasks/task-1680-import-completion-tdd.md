# task-1680 — completing an import: the files, and what they export

## 1. What was asked

> # auto complete for imports/references
>
> In a language, such as typescript, if I start to import, I should see auto complete for files,
> classes, functions, etc that I can import.
>
> Create a tdd, then fully implement.

Two lists, asked for in one sentence. Start writing `import { } from '` and the editor should offer
**the files** the project holds; put the caret between the braces and it should offer **what that
file exports** — the classes, the functions, the constants and the types. TypeScript is named as
the example rather than as the scope, so the mechanism has to be the language's own decision the
way `language.definers` already is, and Unluminate's other three code plugins have to be able to ask for
it.

`task-1677` and `task-1678` built the popup this hangs off: the stem under the caret, the
subsequence match, the Sublime scoring rubric, the five keys, the eight rows, the `Ctrl+Space` that
asks by hand and the `unluminate-cli editor complete` that prints the same list. **None of that changes.**
What this ticket adds is a fifth question asked before the four sources are gathered — *is the
caret in the middle of an import?* — and, when the answer is yes, a different pool of candidates.

## 2. What the surveyed editors do, and which tier this is

### 2.1 The semantic tier: TypeScript's own language service

VS Code's import completion is the TypeScript language service, and it does three separate things
worth telling apart, because only two of them are what the ticket asks for.

**Module specifier completion.** Inside the quotes of `from '…'` the service offers paths. The
list is the files the program can see, and the shape of what it inserts is a setting —
`typescript.preferences.importModuleSpecifier` chooses between `shortest`, `relative`,
`non-relative` and `project-relative`, and `importModuleSpecifierEnding` chooses whether `.js` is
written on the end. That the *shape* is a preference rather than a fact is the useful observation:
there is no single right spelling of a specifier, so an editor has to pick one and say which.

**Named import completion.** Inside the braces the service offers the module's exported names,
with their kinds. It knows them because it has type-checked the module.

**Auto-import.** Completing a name that is *not* imported inserts the `import` line as a side
effect (`includeCompletionsForModuleExports`). This is the famous one, and it is deliberately **not**
in this ticket's scope — §12 says why, and says what it would take.

### 2.2 rust-analyzer

The same three, under different names: path completion inside `use`, item completion after `::`,
and "flyimport", which is auto-import. Its `imports.granularity` and `imports.prefix` settings are
the same admission VS Code's are — where a `use` line should be merged and whether it is written
`crate::`, `self::` or plain is a house style, not a fact.

### 2.3 The syntactic tier, which is the one Unluminate is on

Below the language servers there is a real, shipped, non-semantic tier, and it is where the honest
version of this feature lives.

**Vim's `Ctrl+X Ctrl+F`** completes a **file name** from the filesystem, relative to the current
file or `'path'`. No language knowledge whatever — it reads the disk. It is thirty years old and
people use it constantly, because most of the difficulty in writing an import is remembering how
the folder is spelt.

**VS Code's built-in path IntelliSense** for HTML and CSS is the same thing: `href="…"` and
`@import '…'` offer paths off the filesystem with no language service behind them.

**Vim's `'include'` and `'define'` options**, and the `Ctrl+X Ctrl+I` completion that reads them,
are the same shape as `language.definers`: the *editor* has no idea what an import is, and each
language's settings say what one looks like as a pattern. That is the precedent this design
follows exactly — a language describes its imports as data, and Unluminate does the arithmetic.

So both halves the ticket asks for are reachable without a language server:

| The ticket's ask | What answers it here | How honest it is |
|---|---|---|
| "auto complete for files" | the project's own file list, which `Go to File` and `Find in Files` already walk | exact: these are the files that are really there |
| "classes, functions, etc that I can import" | the target file's definitions, from the same `unluminate_core::symbols` reading that `Go to Definition` uses, filtered to the exported ones | the same tier as `Go to Definition`: a syntactic reading, and it says so |

### 2.4 What was rejected

**A language server client.** `task-1675` §2 already weighed and rejected it, and every reason it
gave holds here: a separate program per language, found on `PATH`, holding gigabytes, and nothing
about it could be a screenshot test because *when* it answers depends on the machine. Import
completion is one of the things that would be better with one, and it is not worth the price of
one.

**Reading `node_modules` so bare specifiers can be offered.** `task-1659` measured what walking it
costs: leaving `target`, `node_modules` and `__pycache__` out took Unluminate's own project from 2,022
files to 618 and the whole search from 60 ms to 20. Undoing that so `import … from 'rea│'` can
offer `react` would make every *other* search slower for one that a person types from memory
anyway. §12 records what a cheaper version would look like if it is ever asked for.

**Auto-import.** §12.

**Parsing the language.** A parser for TypeScript's import grammar would be a second reader of the
same text `unluminate_core::syntax` already reads, and it would drift from it, which is the exact fault
`syntax::scan` was made a visitor to avoid. What is needed is much less than a parser: where the
statement starts, whether the caret is inside its string or inside its braces, and what the module
is spelt as. §4 does that with one backwards walk.

## 3. The shape of the design

One sentence: **when the caret is inside an import, the four sources are replaced by one, and what
that one offers is the project's own files or the target module's own exports.**

Three pieces, in the three places Unluminate already puts these things:

- `unluminate_core::imports` — *what the caret is in the middle of*. Pure: a `&str`, an offset and a
  `Grammar` in, a `Context` out. No disk, no window, tested with no fonts. The sibling of
  `unluminate_core::completion`, which says what a stem is, and of `unluminate_core::symbols`, which says
  what a definition is.
- `unluminate_app::services::imports` — *what that context could become*. This is where the disk is:
  resolving `'./layout'` to a real file, turning a real file into the specifier that would reach
  it, and walking a module path down a folder tree.
- `unluminate_app::app::completion` — one new branch, four lines long: ask `unluminate_core::imports` first,
  and if it answers, gather from `services::imports` instead of from the four sources.

Everything downstream is untouched. The rows are `completion::Row`s, ranked by the same
`completion::rank`, drawn by the same `components::completion`, steered by the same five keys,
accepted by the same `Command::ReplaceMany`, and printed by the same `unluminate-cli editor complete`.

## 4. What the caret is in the middle of

### 4.1 Two families, because there are two shapes of import and no third

```ts
import { Layout, Caret } from './layout';   //  the module is a string
@import 'theme.css';                        //  the module is a string
```
```rust
use unluminate_core::completion::{Candidate, Row};   //  the module is a path of segments
```

A string specifier is resolved against the **file system**, relative to the file being edited. A
segment path is resolved against a **module tree**, which is the file system read through the
language's own rules about where a module lives. They share nothing but the popup, so they are two
readings and one enum:

```rust
pub enum Context {
    /// Inside the module specifier: `from './lay│'`. The range is what would be replaced,
    /// which is the string's content and never its quotes.
    Specifier { typed: Range<usize>, whole: Range<usize> },
    /// Inside the braces of an import whose module is written: `import { Lay│ } from './layout'`.
    Named { module: String, stem: Range<usize> },
    /// Inside a path-family import: `use unluminate_core::comp│`.
    Segment { segments: Vec<String>, stem: Range<usize> },
}

pub fn context_at(text: &str, offset: usize, grammar: &Grammar) -> Option<Context>
```

`whole` on `Specifier` is the whole of the string's content and `typed` is the part of it left of
the caret, which is the `Tab`/`Enter` distinction `task-1677` §5.4 already draws — except that
`completion::word_at` cannot draw it here, because a specifier is not made of word characters and
the grammar would break `./lay/out` into three words. So the range `Tab` replaces comes out of the
context rather than out of the grammar, and that is the only thing about accepting that changes.

### 4.2 The quoted family, read backwards from the caret

Three questions, asked in order, and the first one that answers wins.

1. **Is the caret inside a string on its own line?** Strings in an import do not span lines in any
   of the three languages that have this family, so the line the caret is on is scanned from its
   start, tracking quotes and the grammar's escapes. If it is, the string is the candidate
   specifier; go to step 3.
2. **Is the caret inside an unclosed `{`?** Scanned backwards from the caret, counting braces,
   giving up after [`STATEMENT_LINES`] lines or at a `;`. If it is, the brace is the candidate
   named-import list; go to step 3.
3. **Is this an import statement at all?** Scanned backwards from whichever of the two was found,
   over whitespace, words and `,`, looking for one of `language.import_keywords` as a whole word,
   with no `;` crossed on the way. Nothing found, or a `;` crossed first, and the answer is
   `None` — which is what makes `const path = './layout'` not an import and `export const x = 1`
   not one either.

For the named case the module still has to be read, and it is written **after** the caret
(`import { Lay│ } from './layout'`), so the statement is then scanned **forwards** from the keyword
to its end — the first `;` outside a string, or a blank line, or [`STATEMENT_LINES`] lines,
whichever comes first — and the last complete string literal in it is the module. No string, no
module, no offer: a half-typed `import { Lay│ }` has nothing to be an answer about, and guessing
would be worse than saying nothing.

Why backwards from the caret rather than forwards from the top of the file: the same reason
`symbols::identifier_at` works from the point rather than from a parse. It costs a few hundred
bytes of scanning per keystroke rather than a reading of the file, and a file whose earlier lines
are half-typed cannot poison the answer.

### 4.3 The path family, anchored on its keyword

`use unluminate_core::completion::{Candidate, R│ow};`

One backwards walk from the stem, and the walk *is* the parse:

```
stem      = the word characters left of the caret            ->  "R"
segments  = walk back, and at each step:
              a separator (`::`)  -> take the word before it as a segment
              a `{`               -> step over it and carry on with the outer path
              a `,`               -> skip back over the sibling items to their `{`
              anything else       -> stop
anchor    = the word immediately before where the walk stopped
```

The anchor is what makes this trustworthy. `use` in front and it is an import; anything else in
front and it is `a::b::c` in ordinary code, which must offer the ordinary four sources and not a
module list. There is no separate search for the keyword and no line budget, because the walk
either reaches the keyword or it does not.

`use │` — nothing typed at all — walks zero steps, finds the keyword immediately, and reports
`Segment { segments: [], stem: empty }`. That is the case that offers the roots and the packages,
and it is why the empty stem has to be allowed at all (§5.2).

`use a::b as c│` stops at `as`, which is not the keyword, so it answers `None`. That is right: a
name being bound is not a name being chosen.

### 4.4 The manifest keys

Nine, all of them absent unless a language asks, which is the rule every key added since
`task-1671` has followed and which `the_older_plugins_ask_for_none_of_what_the_imports_added`
keeps. Five are the quoted family's, three are the path family's, and one turns the whole thing on.

| Key | What it says | TypeScript | CSS | Rust |
|---|---|---|---|---|
| `language.imports` | which family, and that there is one at all | `quoted` | `quoted` | `path` |
| `language.import_keywords` | the words a statement begins with | `import, export, require` | `@import` | `use` |
| `language.import_extensions` | what a specifier may resolve to, in order | `.ts, .tsx, .d.ts, .js, .jsx, .mts, .cts` | `.css` | `.rs` |
| `language.import_index` | the basenames a folder's own module is written in, in order | `index` | — | `mod, lib, main` |
| `language.import_omit_extension` | whether the inserted specifier drops the extension | `true` | `false` | — |
| `language.export_keyword` | the word that makes a definition importable | `export` | — | `pub` |
| `language.path_separator` | what joins two segments | — | — | `::` |
| `language.source_roots` | the folder a package's module tree is rooted in | — | — | `src` |
| `language.path_roots` | `word=meaning`, for the segments that are not module names | — | — | `crate=package, self=module, super=parent` |

`path_roots` is checked against the three meanings Unluminate has, exactly as `language.definers` is
checked against the five `SymbolKind`s and `plugin.kind` against the kinds this version loads: a
manifest naming a fourth is refused with a message rather than loaded as a language whose `use`
lines quietly never complete.

Nine keys is more than any previous ticket has added at once, and the alternative was worse. The
alternative is a list of languages inside Unluminate — a `match` on the extension saying that `.ts`
imports look like this and `.rs` imports look like that — which is the exact thing
`language.definers` exists to prevent, and which would mean a plugin somebody writes for Python
could never have the feature at all. Each key is one line of data, each is off by default, and
`plugins::tests` pins that the four plugins that shipped before this ticket are unchanged by it.

## 5. What is offered

### 5.1 The three pools

**A specifier** offers every file in the project whose extension is in
`language.import_extensions`, expressed as the relative specifier that would reach it from the file
being edited: `./layout`, `../core/completion`, `./widgets/button`. The file being edited is not
offered — an import of oneself is not a thing anybody means. `import_omit_extension` drops the
extension, and a file whose basename is one of `import_index` is offered as its folder, so
`widgets/index.ts` is offered as `./widgets` and not as `./widgets/index`.

The specifier is always relative and always starts `./` or `../`. That is the one place this design
makes a choice VS Code makes a setting of, and it makes it because a relative specifier is the one
that is *always right*: it needs no `tsconfig.json` read, no `baseUrl`, no path alias table, and no
`package.json` `exports` map. A specifier this design offers is one that resolves on the disk as it
is now.

**A named import** offers the module's exported definitions, with the kind glyph each already
carries. Where they come from is the ownership rule of `task-1675` §3.3, unchanged: *a file that is
open is owned by its `Document`, and every other file is owned by the index.* An open module's
exports come from its live text through `tab_symbols`, so a function added in the tab beside this
one is offered before it is saved; a closed module's come from `services::symbol_index`.

**A segment** offers, from wherever the path has been resolved to: the child modules there — the
folders and the `.rs` files, minus the ones named by `import_index` — and, when the path has
resolved to a file or to a folder that has a module file, that file's exported definitions. With
no segments at all it offers the `path_roots` words and the packages: every folder under the
project root that holds a `source_roots` folder, named as the module is spelt, which is its folder
name with `-` read as `_`.

### 5.2 The empty stem, which is new

`completion::rank` returns nothing for an empty stem, and it is right to: with nothing typed there
is nothing being completed, and a popup that opened on every space would be unusable. An import is
the exception, and the reason is in the shape of the statement — `from '│'` and `use │` are
positions where **the language itself says what comes next**, so a list is an answer rather than an
interruption. That is IntelliJ's rule as well: its popup opens unasked after a `.` and after
`import`, at zero characters.

So `completion::rank_all` is added beside `rank`: the same function, except that an empty stem
matches everything and the rows come back ordered by source and then by name. `rank` keeps its
guard and keeps its callers. Nothing about the ordinary four sources changes.

### 5.3 A new `Source`

`Source::Module`, for a row that is a file or a module rather than a name inside one. It carries
the kind glyph `SymbolKind::Module` already has, so no icon is drawn for this ticket, and its
detail is the path relative to the project root — `crates/unluminate-core/src/completion.rs` — which is
the sentence a person needs to be sure it is the file they meant.

It sorts first among the sources, which only ever decides a tie, and a tie only happens in the
path family where a module and an item can share a name. A module wins it, because `use a::b` with
`b` both a module and a function is far more often the module.

### 5.4 What accepting does

The same `Command::ReplaceMany`, one undo step, and the same two keys meaning the same two things:
`Enter` replaces what has been typed, `Tab` replaces the whole of it. What differs is only where
"the whole of it" comes from — `Context::Specifier::whole` rather than `completion::word_at`, for
the reason §4.1 gives. A module path segment and a named import are ordinary identifiers, so those
two use `word_at` exactly as every other row does.

Accepting does **not** add a closing quote, a comma, a `from` clause or a `;`. Unluminate's editing area
has never inserted a character nobody typed, and a completion is not the place to start.

## 6. Resolving a module

### 6.1 A specifier to a file

```
'./layout'  from  src/app/mod.ts
  ->  src/app/layout        + each of import_extensions      -> src/app/layout.ts        ✔
  ->  src/app/layout/index  + each of import_extensions      -> src/app/layout/index.ts
```

The folder is the edited file's own, `.` and `..` are applied, and the candidates are tried in the
order the manifest lists them, so `.ts` beats `.js` because TypeScript's manifest says `.ts` first.
A specifier with a scheme in it, or one that does not start with `.`, resolves to nothing — the
first because `services::preview_images` already established that nothing in Unluminate fetches, the
second because a bare specifier is a package and §2.4 says why packages are not offered.

The resolution is checked against the project's own file list rather than against the disk, which
means it costs no `stat` and cannot see a file outside the project. That is the same list
`Go to File` searches, so a file the search can find is a file an import can resolve to, and there
is one answer to "what files are in this project" rather than two.

### 6.2 A segment path to a folder or a file

```
crate::app::actions        from  crates/unluminate-app/src/components/editor_view.rs
  crate     ->  the source root above this file      ->  crates/unluminate-app/src
  app       ->  a folder or an `app.rs`              ->  crates/unluminate-app/src/app
  actions   ->  a folder or an `actions.rs`          ->  crates/unluminate-app/src/app/actions.rs
```

```
unluminate_core::completion
  unluminate_core  ->  a package folder whose name reads `unluminate_core`, and its source root
                                                    ->  crates/unluminate-core/src
  completion  ->                                    ->  crates/unluminate-core/src/completion.rs
```

`self` is the module the file is, `super` is the one above it, and repeated `super::super::` walks
up. A segment that resolves to **both** a file and a folder — `app.rs` beside `app/`, which is how
Rust's older module style is written — resolves to both, and what is offered is the union: the
file's exports and the folder's children. Showing both rather than guessing one is the rule
`task-1675` set for a name defined twice, and it applies unchanged.

A segment that resolves to nothing ends the walk, and an unresolved path offers nothing. There is
no partial credit: offering the whole project's `pub` items because the first segment was a typo
would be a list that is never right.

## 7. What it costs

The budget is `task-1678`'s, unchanged: **a keystroke under 5 ms on the largest file in this
repository**, measured by `cargo run --release -p unluminate-app --example completion_cost`. Import
completion has to fit inside it, and three things about it are worth writing down.

**Nothing new is indexed and no thread is added.** The file list is `FileTree::all_files`, which
the explorer already holds; the exports are `services::symbol_index`, which already has its own
worker; an open module's are `tab_symbols`, already cached on the tab and keyed on
`text_revision()`. This ticket reads three things that are already in memory, which is the same
sentence `task-1677` §4.1 opens with and is why this is a small feature.

**The one unbounded moment is the empty stem.** `from '│'` builds a relative specifier for every
importable file in the project — 618 files on Unluminate's own repository, and a `.ts` project of that
size would be similar. Each is a little path arithmetic and one `String`. It happens once per
import statement, and the very next character typed makes `completion::could_match` throw nearly
all of them away before a `Candidate` is built, which is the prefilter `task-1678` added for
exactly this reason. `completion_cost` grows an import arm so the number is measured rather than
assumed.

**The index gains one table.** `Index::exports_of(path)`, so a closed module can answer without
being read. It holds only the **exported** definitions, which is what makes it small: it is a
second copy of a subset of names the table already holds, and on Unluminate's own repository that is
some thousands of short strings. The alternative — reading the module off the disk on each
keystroke — is 1.8 ms for the largest file in this repository and would be paid on every letter
typed between the braces.

### 7.1 What it measured

`cargo run --release -p unluminate-app --example completion_cost`, run against Unluminate's own repository
after the implementation:

| Inside an import | offered | total |
|---|---|---|
| `use ` | 12 | 0.345 ms |
| `use unluminate_core::` | 14 | 0.874 ms |
| `use unluminate_core::completion::` | 12 | 0.530 ms |
| `use unluminate_core::completion::Can` | 1 | 0.579 ms |

So the import arm's worst case is **0.87 ms**, comfortably inside the 5 ms a keystroke has, and the
unbounded moment turns out not to be the expensive one.

Two numbers about the ordinary arm are worth recording, because the instrument prints them and a
reader will wonder.

**The export marker costs 0.000 ms.** It is one look at the text between the export keyword and the
token in front of it, and only while a marker is armed. The example now prints the difference
against the same file with `language.export_keyword` taken away, so it stays measured rather than
assumed.

**The whole-keystroke figure has drifted from `task-1678`'s 4.59 ms to 5.06 ms, and none of it is
this ticket's.** The instrument measures the largest file a language claims, which is
`tests/screenshots.rs`, and that file has grown to 271 KB. The 5.06 is a **one** character stem,
which only `Ctrl+Space` can ask for, because the automatic popup does not open under two; the two
character stem the popup really opens on is **4.42 ms**. It is recorded here rather than fixed:
capping the pool is a change to what `task-1678` offers, it was not asked for, and the honest way to
raise it is its own ticket with its own before and after.

## 8. Fitting into the window

**The gate.** `file_kind::completion_applies` already asks whether a plugin claims the file. The
import branch asks one more question, `Grammar::completes_imports`, which is true when
`language.imports` was named. A language that did not ask has exactly the behaviour it has today,
to the byte.

**The trigger.** The automatic popup's rules are `task-1677` §5.1's, with two changes and no others.
`AUTOMATIC_STEM` — two characters before the list arrives unasked — does not apply in an import
context, for §5.2's reason. And the role check does not apply: `offer_a_completion` refuses to open
inside a comment or a string, and a module specifier **is** a string, so the check is asked of the
import context first. Neither change can affect a file whose language named no imports.

**Reopening in the same frame.** `refresh_the_completion` closes the popup when the stem's start
moves, which is what typing `::` does: a new segment is a new stem. So `keep_the_completion_fresh`
gains one line — if the refresh closed the list and a character was typed this frame, ask again.
Without it, `use unluminate_core::│` would need one more keystroke before the list came back. It changes
nothing outside an import, because `offer_a_completion` still refuses a stem shorter than two
characters.

**The action and the setting.** Both already exist and neither changes. `Complete Word` /
`Ctrl+Space` asks by hand and now answers inside an import too, which is what a person who has just
typed `from '` and waited will press. `editor.suggestions = manual` still turns the automatic popup
off everywhere, imports included.

**The command line.** `unluminate-cli editor complete` prints whatever the popup would show, and gains
nothing but the ability to print an import list — its one guard, "there is nothing to complete
here" when the stem is empty, becomes "when the stem is empty *and* there are no rows", because an
empty stem inside a specifier is now a real question with a real answer. `--choose` applies an
import row exactly as it applies any other. No new command, no new flag, no change to
`docs/commands.md` beyond the summary sentence for `editor complete`.

## 9. Where the state lives

Nowhere new. `CompletionState` gains one field — the `Context` the rows were worked out for, so
that accepting knows whether `Tab` means `word_at` or `Specifier::whole`, and so the popup does not
have to re-parse to find out. It is `Option<Context>`, `None` for every ordinary completion, which
is what makes the change invisible to the four sources.

Nothing is written to disk, nothing is remembered between runs, and no setting is added.

### 9.1 The one thing in the model that had to change

`Command::ReplaceMany` **dropped an empty range**, and accepting a row inside `from '│'` replaces no
bytes and inserts all of them — so the first working version of this feature typed nothing at all
when the quotes were still empty.

The fix is one line of the filter and it is the right shape rather than a special case: *an empty
range carrying text is an insertion at that point.* An empty range carrying nothing is still
dropped, because it would push an undo step for an edit that changes no byte. Everything else about
the command is untouched, `remove_range` already returned early on an empty range, and accepting an
import is therefore the same single `ReplaceMany` — and so the same single undo step — that every
other completion has always been.

## 10. The scenario battery

Fifty-one, which the implementation is held to. The numbering is what the tests cite.

### 10.1 Reading the context — the quoted family

1. `import { A } from './la│'` is a `Specifier` whose typed part is `./la`.
2. `import { A } from '│'` is a `Specifier` with nothing typed.
3. `import A from "./la│"` — a default import, and double quotes.
4. `import * as ns from './la│'` — a namespace import.
5. `export { A } from './la│'` — a re-export.
6. `const x = require('./la│')` — a keyword that is not at the start of a line.
7. `@import 'the│'` in CSS.
8. `@import url("the│")` in CSS.
9. `const path = './la│'` is **not** an import: no keyword before it.
10. `import { A } from './a'; const p = './b│'` is not an import: a `;` was crossed.
11. `import { La│ } from './layout'` is a `Named` whose module is `./layout`.
12. `import {\n  La│\n} from './layout'` — the same across three lines.
13. `import { A, La│ } from './layout'` — a second name in the list.
14. `import type { La│ } from './layout'` — a word between the keyword and the brace.
15. `import { La│ }` with no `from` clause yet offers nothing: there is no module to be about.
16. A brace opened more than `STATEMENT_LINES` lines above the caret is not an import.
17. The caret inside a string that is inside braces — `import { a } from './x'` typed inside out —
    reads as a `Specifier`, because the string question is asked first.
18. A language that named no `language.imports` never has a context, whatever the text says.

### 10.2 Reading the context — the path family

19. `use unluminate_core::comp│` is a `Segment` with segments `[unluminate_core]` and stem `comp`.
20. `use │` is a `Segment` with no segments and no stem.
21. `use unluminate_core::│` is a `Segment` with segments `[unluminate_core]` and no stem.
22. `use unluminate_core::completion::{Candidate, R│}` has segments `[unluminate_core, completion]`.
23. `use unluminate_core::{a, b::c│}` has segments `[unluminate_core, b]` — the sibling list is stepped over.
24. `pub use unluminate_core::comp│` reads the same as `use` does.
25. `let x = a::b::c│` is **not** an import: the anchor is not the keyword.
26. `use a::b as c│` is not a context: a name being bound is not a name being chosen.
27. A `::` inside a comment or a string is not a path, because the anchor walk never reaches a
    keyword through one.

### 10.3 Resolving

28. `./layout` from `src/app/mod.ts` resolves to `src/app/layout.ts`.
29. `./widgets` resolves to `src/app/widgets/index.ts` when there is no `widgets.ts`.
30. `../core/completion` resolves through a parent folder.
31. `.ts` is preferred to `.js` when both exist, because the manifest lists it first.
32. `react` resolves to nothing: it is not relative.
33. `https://example.com/a.js` resolves to nothing: it has a scheme.
34. `./layout.css` from a stylesheet resolves with the extension written out.
35. `crate::app::actions` resolves from a file under `crates/unluminate-app/src`.
36. `unluminate_core::completion` resolves through the package folder whose name reads `unluminate_core`.
37. `super::super::x` walks two modules up.
38. `self::parts` resolves inside the file's own folder.
39. A segment that is both `app.rs` and `app/` offers the union of both.
40. An unresolved first segment offers nothing rather than everything.

### 10.4 What is offered, and what accepting does

41. A specifier list holds `./layout` and not `./layout.ts`, because the manifest omits extensions.
42. A specifier list never holds the file being edited.
43. A specifier list holds a file in a folder as `./widgets/button`.
44. A named list holds only the module's **exported** definitions — `export const` yes, a `const`
    inside a function no.
45. A named list of an **open** module comes from its live text: a function added in the tab beside
    this one is offered before it is saved.
46. A named list of a closed module comes from the index.
47. A Rust segment list holds only `pub` items.
48. `Enter` on `./la│yout` replaces `./la`; `Tab` replaces `./layout`, and neither touches the
    quotes.
49. Accepting is one undo step, and one press of undo puts back exactly what was typed.
50. Typing `::` reopens the list on the new segment in the same frame.
51. The four ordinary sources are **not** offered inside an import: no keyword, no local word, no
    unrelated project name.

### 10.5 The properties that hold everywhere

- Laying the same text and the same caret out twice gives the same rows in the same order.
- Nothing in this ticket reads the disk on a frame where the text did not change.
- A file whose language named no imports behaves exactly as it did before this ticket, byte for
  byte, which `the_older_plugins_ask_for_none_of_what_the_imports_added` and the existing
  completion tests together assert.

## 11. What is built where

| Where | What |
|---|---|
| `unluminate-core/src/imports.rs` | new. `Context`, `context_at`, and the two backwards walks. |
| `unluminate-core/src/syntax.rs` | `Grammar` gains the nine import fields and `completes_imports`. |
| `unluminate-core/src/symbols.rs` | `Definition` gains `exported`, set by `language.export_keyword`. |
| `unluminate-core/src/completion.rs` | `Source::Module`; `rank_all` beside `rank`. |
| `unluminate-app/src/services/plugins.rs` | reads the nine keys; refuses an unknown `path_roots` meaning. |
| `unluminate-app/src/services/imports.rs` | new. Resolving a specifier and a segment path, and listing what each could become. |
| `unluminate-app/src/services/symbol_index.rs` | `Entry` gains `exported`; `Index` gains `exports_of`. |
| `unluminate-app/src/app/completion.rs` | the import branch, the trigger changes, and the accept range. |
| `unluminate-app/src/app/cli.rs` | the one guard that has to allow an empty stem with rows. |
| `unluminate-app/plugins/*/plugin.conf` | TypeScript, JavaScript, CSS and Rust say how their imports look. |
| `unluminate-app/tests/screenshots.rs` | a TypeScript fixture project and two pictures. |
| `unluminate-app/examples/completion_cost.rs` | an import arm, so §7's number is measured. |

## 12. What is deliberately not here

**Auto-import.** Completing `Layou│` in the body of a file and having `import { Layout } from
'./layout'` appear at the top is the third thing §2.1 lists, and it is a different feature with a
different risk: it edits a part of the file the caret is not in. Doing it honestly needs three
things this ticket does not build — knowing which names are *already* imported so a second import
is not written, knowing where the import block ends so the new line goes in the right place, and
knowing which of several files exporting `Layout` was meant. The first two are reachable from what
is here; the third is the one a language server answers and this tier cannot. It is a good follow-up
and it should be its own ticket.

**Bare package specifiers.** §2.4. If it is ever asked for, the cheap version is to read the names
out of the project's own dependency manifest — `package.json`'s `dependencies`, `Cargo.toml`'s
`[dependencies]` — which is a few hundred bytes read once per project and never a walk of
`node_modules`. It would need a manifest key naming the file and a reader for two formats, which is
more than this ticket should carry.

**Path aliases.** `tsconfig.json`'s `paths`, `baseUrl`, and Vite or webpack aliases. A specifier
offered through an alias is one this design cannot verify resolves, and §5.1's whole argument is
that what is offered is what is really there.

**Re-exports.** `export * from './other'` means `./other`'s names are importable from this module,
and this design will not offer them, because following a re-export is a graph walk with cycles in
it. What is offered is what the module itself declares. A module that is only a barrel file will
offer its `export { … } from` names, because those *are* written in it, and nothing more.

**Sorting or merging the `use` block.** rust-analyzer's `imports.granularity`. Not asked for, and
it would edit lines nobody is typing on.

## 13. Sources

- TypeScript `importModuleSpecifier`, `importModuleSpecifierEnding` and
  `includeCompletionsForModuleExports` — the three settings that admit a specifier's spelling is a
  preference (code.visualstudio.com/docs/typescript/typescript-editing, and the TypeScript
  `UserPreferences` interface).
- rust-analyzer `imports.granularity`, `imports.prefix`, and flyimport
  (rust-analyzer.github.io/manual.html).
- Vim `Ctrl+X Ctrl+F` file-name completion, and `'include'`/`'define'` with `Ctrl+X Ctrl+I` — the
  syntactic tier, and the precedent for describing a language's imports as data
  (`:h compl-filename`, `:h include-search`).
- VS Code's built-in path completion for HTML and CSS, which needs no language service.
- `tasks/task-1675-code-editing-tdd.md` §2 and §3.3 — why the tier is syntactic, and the ownership
  rule this ticket inherits.
- `tasks/task-1677-autocomplete-tdd.md` §4.1, §5.1 and §5.4 — the sources, the trigger and the two
  acceptance keys, all of which this ticket extends rather than changes.
- `tasks/task-1659-search-and-images-tdd.md` — the measured cost of leaving `node_modules` out of
  the walk.
