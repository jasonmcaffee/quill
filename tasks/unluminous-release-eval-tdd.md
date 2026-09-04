# Unluminous: what stands between it and a general release

`task-1803`. A review of testing, functionality, agent integration, code organisation, user
experience and performance, against the bar of a product people who are not Jason install and use.

Everything below was **measured on this machine on 2026-09-04** against `v0.34.2` — the suite run,
the frame costs timed, the defects reproduced through `unluminous-cli` driving a real window. Where a
number appears it was taken, not estimated, and the command that took it is written beside it so it
can be taken again.

---

## 0. The short version

Unluminous is much closer to a release than a project eleven days old has any right to be. 180,308
lines of Rust across seven crates and a command-line client, 201 commits, **2,679 tests that all
pass**, a design system with a closed palette, a command catalogue of 159 commands that generates its
own MCP tools and fails a test the moment one is missing, and a cold start of **969 ms** to a window
that answers.

It is not ready, and the reasons divide cleanly into two piles that want different work.

**The first pile is small and sharp: seven defects, three of which lose or misreport a person's
data.** A CRLF file is silently rewritten to LF on save. `tab open` reports success for a file it did
not open. The project index has no ignore rules, so definitions and searches answer out of build
output and scratch folders. None of these is architectural; each is a day or less.

**The second pile is the shape of the product rather than a fault in it.** There is no Find in the
current file. There is no Replace, anywhere. There is no code signing on the Windows installer, no
CI, and no way for an installed copy to learn that a newer one exists. An editor that ships without
Ctrl+F is an editor that gets one review.

There is also one thing that is **excellent and should be protected**: the agent surface. The rule
that every feature is reachable by an agent is real, enforced by tests, and measured by a harness
that watches a local model actually use it. The three gaps in it named in §4 are gaps *in a thing
that works*, which is a much better position than most editors are in.

**The order of work, and why:**

| | Why it comes here |
|---|---|
| **A. The seven defects** (§7) | Two of them corrupt files. They are cheap and they are the only items that make Unluminous worse than a text editor from 1995. |
| **B. Find and Replace** (§3.1) | The single most-used keystroke in any editor is not bound. Nothing else in the functionality pile is close. |
| **C. Release engineering** (§6) | CI, signing, an update path. Without these the first pile can be fixed and nobody receives the fix. |
| **D. Typing cost on large files** (§5.2) | 90 ms a keystroke at 2 MB. The fix is written down already in the performance TDD. |
| **E. The agent gaps** (§4) | Real, but the surface works; these make it work for more of the product. |
| **F. Accessibility** (§3.4) | The largest single piece of work here, and the one most likely to be a hard requirement wherever this is sold. |

---

## 1. What was measured, and how

| | Command | Result |
|---|---|---|
| Whole workspace, non-UI crates | `cargo test --workspace --exclude unluminous-app` | **1,295 passed, 0 failed**, ~15 s of test time |
| `unluminous-app`, no window | `cargo test -p unluminous-app --lib --bins` | **907 passed, 0 failed**, 2.32 s |
| Screenshot layer, through `wgpu` | `cargo test -p unluminous-app --test screenshots` | **476 passed, 0 failed**, 134.40 s |
| The board's end-to-end tests | `--test agent_board` | 1 passed, **6 ignored** |
| Build of every test binary | `cargo test --workspace --no-run` | clean, 40.72 s, 4 warnings |
| Screenshot flakiness | the same suite, **four consecutive runs** | 476/476 every time; 134.4 s, 136.4 s, 134.5 s, 137.5 s |
| Frame cost, 500 KB file | `--example frame_cost -- app/mod.rs` | §5 |
| Frame cost, 2 MB file | the same, on a file built by repeating it | §5 |
| Cold start | `unluminous-cli launch`, polled until `instances` answered | **969 ms** |
| Memory, one window on this repo | `Get-Process` after the index settled | 234 MB working set, 422 MB private, 35 threads |
| MCP preamble | `unluminous-cli mcp tools --count` | 24 tools / 17,275 tokens; 164 tools / 42,753 tokens |

Everything in §7 was reproduced by driving a real window with `unluminous-cli`, not by reading the
source. The evidence files are in
`ai-service/_agent_output/task-1803-unluminous-release-eval/`.

---

## 2. Testing

### 2.1 The state of it

2,679 tests, all green, in about two and a half minutes of wall clock. For a graphical editor that is
a genuinely unusual position, and the reason is that the architecture was built to make it possible:
six of the seven crates have no user-interface dependency at all, so the editor's text buffer, the
terminal's emulation, git, the debug adapter protocol, the database wire protocols and the chat
client are all ordinary unit tests. 1,295 of the 2,679 never touch a window.

Three things are worth calling out as done right, because the temptation in the work below will be to
weaken them:

- **The screenshot layer is not flaky any more.** The old figure was an access violation on about one
  run in nine, from `egui_kittest` building a `wgpu` device per harness. The shared device pool fixed
  it, and **four consecutive full runs here were 476/476 with no crash**. Do not let a later change
  reintroduce a per-harness device; the test-file `builder()` is what stops it and it should stay the
  only way to make a harness.
- **The documentation is a test.** `unluminous-cli/src/documentation.rs` fails while a command has no
  section in `commands.md`, while a usage line is stale, or while a section describes a command that
  has gone. Almost nothing else in this industry does that.
- **The MCP tools cannot drift from the catalogue**, because they are generated from it and
  `every_command_is_offered_as_a_tool_in_both_shapes` fails if one is ever held back.

### 2.2 What is not tested

**The six end-to-end agent-board tests never run.** `crates/unluminous-app/tests/agent_board.rs`
holds the tests that start a real agent, walk the watchdog against a board on disk, and prove a
ticket's conversation survives its terminal being closed — including, by its own comment, *"the test
that would have caught the fault `task-28` reported"*. Every one is `#[ignore]`d, guarded by a test
that fails if any is not, and the reasons given are sound: they cost money and minutes. But the
consequence is that **the deepest tests in the repository have no scheduled run at all**, and a
release is exactly the moment they should have one. They belong on a nightly, not on `cargo test`.

**Fourteen files over 400 lines have no test in them.** The largest are `app/mod.rs` (9,996 lines),
`services/database/mod.rs` (1,371), `components/agent_tasks/listings.rs` (1,303),
`components/settings_dialog.rs` (1,077) and `components/explorer.rs` (894). Some of that is fair —
a drawing component's test is a screenshot, and those exist — but `app/mod.rs` is not a drawing
component, and neither is `services/database/mod.rs`. See §8.

**Nothing tests what a file is on disk after a round trip.** This is the gap that let §7.1 through:
there is no test that opens a file with CRLF line endings, edits it, saves it and compares the bytes.
The `file_move` path has exactly this rule written into its own comment — *"encodings, line endings
and trailing whitespace survive byte for byte"* — and the ordinary save path does not keep it.

**One test runs for over a minute on its own.**
`the_shapes_of_javascript_that_could_be_mis_sliced_survive_being_typed` announced itself at 60
seconds on every one of the four runs. It is not failing, but it is a quarter of the suite's clock in
one test and worth a look.

### 2.3 There is no continuous integration

There is no `.github/` directory. Every number in §1 exists because a person ran the commands. That
is workable for one developer on one machine and it is not workable for a release: nothing catches a
change that breaks the macOS build, nothing runs the suite on a machine that is not this one, and
nothing stops a regression reaching a tag.

The platform split makes this more than a formality rather than less. The snapshots are
per-platform — macOS reads `tests/snapshots`, Windows `tests/snapshots/windows` — so **each set is
only ever verified on the machine that happens to be running**, and a change that breaks the other
platform's rendering is found by whoever next opens the other platform.

---

## 3. Functionality

### 3.1 What an editor is expected to have and this one does not

Against the reference editors the README compares itself to, these are absent. The first two are the
release blockers.

| | State | Why it matters |
|---|---|---|
| **Find in the current file** (`Ctrl/Cmd+F`) | **absent** — there is only `Find in Files` on `Ctrl+Shift+F` | The most-pressed key combination in any editor. Its absence is the first thing every reviewer will find, in the first minute. |
| **Replace** — in the file, or across the project | **absent entirely** | `editor rename` renames a *symbol*, which is a different and better thing, and it does not help with a string, a comment, a URL or a number. |
| Multiple carets / column selection | absent | Expected by anyone coming from the reference editors. |
| Bracket matching, auto-indent, auto-closing pairs | absent | The everyday texture of typing code. |
| Minimap | absent | Fine to omit; noted for completeness. |
| Language Server Protocol | absent, by design | Completion and definitions come from the tokeniser and a symbol index instead. That is a real design choice with real benefits — nothing is indexed, nothing is started, it works on any file the moment it is opened — but it means no diagnostics, no types on hover, and no cross-language accuracy. It should be **stated in the README as a choice**, because a reader who assumes an LSP and finds none reads it as missing rather than decided. |

### 3.2 What is genuinely strong

It is worth being clear about the size of what is already here, because the list above reads harsher
than the product is. Shipped and tested: git in full (status, blame, log, diffs, branches, commit),
a terminal with tabs running the person's real login shell, a debugger over DAP with breakpoints,
stepping, stack frames and value tooltips, run configurations, folding, a Markdown preview that draws
tables and pictures and scrolls with its source, Mermaid diagrams laid out in Rust, a browser tab on
WebView2/WKWebView with a project-scoped origin, a database explorer speaking PostgreSQL's v3 wire
protocol and SQLite, an agent chat pane driving the `claude` and `codex` binaries already installed,
five themes, a plugin system that executes nothing, panel docking to any edge, split panes, symbol
rename across a project, import-aware completion, and an installer for both platforms.

That is more than most editors have at 1.0.

### 3.3 File handling is the weak seam

Three of the seven defects in §7 are here, and they share a cause: **a file is a `String` and nothing
remembers what it was on disk.** Line endings are normalised on read and lost on write; a file that
is not UTF-8 cannot be opened at all and the CLI does not admit it; a lone `\r` is not a line break.
The fix is one small type — what was read, kept beside the text — and §7 says so in each place.

### 3.4 Accessibility is absent

There is no AccessKit tree, no screen-reader support, and no mention of contrast, colour-blindness or
WCAG in the 458-line style guide. Every control is painted, and `components/agent_tasks/ticket_modal.rs`
contains the one comment in the tree that notices the accessibility tree exists — to explain that a
control was *painted rather than named* to stay out of it.

egui supports AccessKit, so this is reachable rather than architectural, but it is the largest single
item in this document and it is the one most likely to be a hard requirement wherever the product is
sold. It should be scoped now even if it ships after 1.0.

There is also no localisation of any kind. That is a reasonable thing to defer; it should be a
decision written down rather than an omission.

---

## 4. Agent integration

### 4.1 The claim is real, which is the headline

*"Everything a person can do in this window, an agent can do too, through the same command, and both
are covered by automated tests"* is not marketing. It is enforced:

- 159 commands over 20 areas in one catalogue, and `run_cli` is the single place a command becomes a
  change, exactly as `run_action` is for the menus — so an agent and a person reach the same code.
- Menu entries need nothing: `actions::menus` is walked to build `action list`, and
  `app/action_names.rs` fails when an entry has no name.
- The MCP tools are generated from the catalogue, and two tests fail if a command is ever not offered
  or if the exclusion list grows past its one member.
- `commands.md` is verified by a test, so the document handed to an agent cannot go stale.
- And `tools/agent-study/` drives a **real local model** — Qwen 3.8 27B on llama.cpp — through 23
  scenarios phrased the way a person speaks, grading what happened by reading Unluminous's own state
  back rather than by believing the agent. That is the question almost nobody asks: not *can* an
  agent reach it, but *does* it.

`task-1695` and `task-1699` then acted on what that harness found — payloads made proportionate,
`action list --menu` added to narrow an answer, narrow semantic tools added so a dedicated Unluminous
answer competes with a generic `grep`. This is the strongest part of the product and the work below
should not disturb it.

### 4.2 The three gaps

**The newest two features are configurable by a person and not by an agent.** `settings list` names
23 keys and none of them belong to the Agent-Chat or the Database plugin. A person sets a chat row's
program, URL, model and key-variable, and a data source and where its password lives, through
Settings pages; an agent has no route to any of it. Both plugins expose only their coarse menu verbs
through `plugins run` — `open-pane`, `new`, `stop`, `tools` for chat; `add-source`, `new-table`,
`reload`, `submit` for the database — so an agent cannot send a message and read the reply, and
cannot run a query and read the result. Against the rule the product is built on, these are the two
places it is not kept.

**The tool preamble is a fifth of a local model's context.** By the CLI's own count:

| Shape | Tools | Bytes | Tokens | Of the study's 96k window |
|---|---|---|---|---|
| `grouped` (default) | 24 | 69,100 | **17,275** | **18%** |
| `every` | 164 | 171,014 | **42,753** | **45%** |

Spent before a question is asked. `mcp serve` takes `--tools grouped|every` and nothing else, so
there is no way to equip an agent with, say, the editor and git areas and leave the database, the
debugger and the browser out. For the local lane, which is where this product's agent story is
strongest, that is the dominant cost of using it.

**The study has not grown with the product.** Its 23 scenarios were last changed in substance on
2026-08-28. Browser tabs (`task-1756`), the database plugin (`task-1777`) and the agent chat pane
(`task-1767`) all landed after, and none has a scenario. `CLAUDE.md` says *"Add a scenario when you
add a feature. A feature nobody has watched an agent use is a feature nobody knows is reachable in
practice"* — and three features are now in exactly that state.

### 4.3 And one defect that matters more here than anywhere

§7.2 — `tab open` answering `ok: true` for a file it did not open — is an ordinary bug for a person,
who sees the reason in the status bar. For an agent it is the worst kind of bug there is: it is told
the file is open, it is given a tab number, the process exits 0, and every subsequent step operates
on whatever was there before. The product's whole argument is that an agent can trust these answers.

---

## 5. Performance

### 5.1 What is fast

Cold start to a window answering a command is **969 ms**, on a 30 MB binary opening a 180k-line
project. Memory is 234 MB working set with the project indexed. Scrolling and selection are properly
culled and stay flat as the file grows:

| | 500 KB / 10k lines | 2 MB / 40k lines |
|---|---|---|
| dragging a selection | 0.07 ms (14,541 fps) | 0.07 ms (14,646 fps) |
| scrolling | 0.04 ms (28,477 fps) | 0.04 ms (27,269 fps) |
| glyphs, one screenful | 0.06 ms (1,843 glyphs) | 0.05 ms (1,843 glyphs) |

That flatness is `task-1666`'s work and it held exactly as designed: four times the file, the same
cost, because the painter places what is on the screen rather than what is in the file.

### 5.2 What is slow, and it is the one the last TDD predicted

| | 500 KB / 10k lines | 2 MB / 40k lines |
|---|---|---|
| **typing a letter** | 9.13 ms (110 fps) | **37.76 ms (26 fps)** |
| **typing, coloured again** | 23.52 ms (43 fps) | **90.35 ms (11 fps)** |
| first layout of the whole file | 92.43 ms | 343.18 ms |
| syntax highlight | 5.28 ms | 19.52 ms |
| `set_syntax` | 5.45 ms | 25.86 ms |

Four times the file, four times the cost per keystroke: **linear in file size, when it should be
linear in the size of the edit.** At 2 MB every character typed costs 90 ms, which is not a slow
editor, it is an unusable one.

`tasks/task-1666-performance-tdd.md` §12 named this precisely and in advance — *"The tokeniser reads
the whole file after every edit… it is the next thing to become the largest item"* — and gave the
fix: tokenise from the start of the line the edit was on and stop once the tokens agree with what was
there before, which is the rule `relayout` already follows for layout. **Nothing new needs designing;
this is the plan being carried out.**

Two smaller items from the same section are worth doing beside it. Opening a 2 MB file costs 343 ms
of layout on the drawing thread, and there is no size guard and no progress — the window simply stops
for a third of a second, longer on a slower machine. And `line_of_offset` and `line_at_y` measured at
0.0000 ms at both sizes, so nothing there needs touching.

### 5.3 The measuring instrument is good and should stay

`cargo run --release -p unluminous-app --example frame_cost -- <file> [width]` produced every number
above in one command, using the real fonts of this machine and the real colouring. A performance
claim in this project can be checked in ten seconds. Keep it, and add the two file sizes above to
whatever runs before a release.

---

## 6. Release engineering

This is the pile with the least code in it and the most consequence, because it decides whether any
of the rest reaches anybody.

**The Windows installer is not signed.** `installer/windows/build.ps1` never calls `signtool` and
`unluminous.iss` has no signing directive. Every download therefore meets a SmartScreen warning
saying Windows protected your PC, and the only way past it is *More info → Run anyway*. macOS is
handled properly — `installer/macos/build.sh` supports ad-hoc, Developer ID and full notarisation
with `notarytool` and stapling — so this is a gap on one platform, not a missing idea. It needs a
code-signing certificate, which is a purchase and a lead time, so it should be started before it is
needed.

**There is no update path.** Nothing in the binary asks whether a newer version exists. A person who
installs 0.34.2 stays on 0.34.2 until they happen to visit the releases page. For a product that
releases on every finished task this is the largest single gap between "we shipped a fix" and
"someone has the fix". The `releases/` folder and the GitHub release the tooling already publishes
are most of the machinery; what is missing is a check and a prompt.

**There is no CI.** Covered in §2.3.

**No CHANGELOG.** 201 commits and 34 minor versions with no record of what changed between them that
a user could read. The commit history is greppable by ticket, which serves a developer and not a
person deciding whether to update.

**No LICENSE file at the repository root.** The terms exist and are correct — `Cargo.toml` declares
`MIT OR Apache-2.0` and `installer/windows/license.txt` reproduces the MIT text — but the
conventional `LICENSE-MIT` and `LICENSE-APACHE` files are not there, which is where every tool and
every reader looks first.

**The documentation gallery is stale, and says so.** `documentation/overview.md` was captured from
0.1.0 and lists what it is missing: `Go to File`, `Find in Files`, pictures in the Markdown preview,
everything `task-1685` did, and themes — which change every colour in every picture. Add the browser,
the database and the chat pane to that list. A gallery that shows the wrong icon set and the wrong
colours is worse than none on a download page.

**There is no first-run experience.** Nothing greets a person who opens Unluminous for the first
time and nothing tells them the keyboard shortcuts exist. The README is 1,041 lines, which is a
developer's document.

---

## 7. The defects, with what was measured

Each of these was reproduced through `unluminous-cli` against a running window.

### 7.1 A CRLF file is silently converted to LF on save — **data corruption**

`Document::open` calls `read_to_normalised_string`, which does `text.replace("\r\n", "\n")`.
`Document::save_as` writes `self.text.to_string()`. Nothing remembers what the file was.

Measured. A three-line file written with `\r\n`, opened, one character typed, `action run save`:

```
before:  l i n e   o n e \r \n l i n e   t w o \r \n l i n e   t h r e e \r \n
after:   X l i n e   o n e \n   l i n e   t w o \n   l i n e   t h r e e \n
```

Every line ending in the file was rewritten by a one-character edit. On Windows — and on this machine,
whose `core.autocrlf` is set, as `CLAUDE.md` itself notes — a git checkout is enough to put every file
in the working tree into this state, so **any edit to any file produces a whole-file diff.** On a
repository not using `autocrlf` the conversion is permanent.

The normalisation itself is right and `task-1794` explains why: offsets and line counts need one
meaning, and a breakpoint sent to a debugger from a file read raw was landing about fifty bytes early.
The fault is only that the write does not undo it.

**Fix.** Keep what was read on the `Document` — one enum, `Lf` or `Crlf`, decided by what dominates
the file on open, defaulting to the platform's for a new file — and apply it in `save_as`. Add a
Settings key so a person can force one, and put the current file's ending in the status bar beside
its kind. **Test:** a round trip that compares bytes, for each of CRLF, LF, and a file with no
trailing newline (that last one is already correct and should be pinned).

### 7.2 `tab open` reports success for a file it did not open — **false success**

Measured, on a file holding Latin-1 bytes:

```
$ unluminous-cli --json tab open latin1.txt
{ "command": "tab.open", "ok": true, "tab": 0,
  "message": "Opened ...latin1.txt in tab 0" }          exit 0
$ unluminous-cli --json tab list
  ...no such tab...
$ unluminous-cli window message
  Unluminous could not open ...latin1.txt: stream did not contain valid UTF-8
```

A file that does not exist is refused correctly (`ok: false`, `not-found`, exit 1). The failure is
specific to a file that exists and cannot be decoded: `open_path_in_tab` sets `self.message` and
returns, and the CLI reply is built without asking whether the tab is there.

**Fix.** `open_path_in_tab` returns a `Result`, and `run_cli`'s `tab.open` arm answers `ok: false`
with the reason. **Test:** every command that can fail after reporting is checked the same way —
assert on the state, not on the reply.

### 7.3 The project index has no ignore rules — **wrong answers**

The only thing skipped is three hardcoded folder names, in `services/file_tree.rs`:

```rust
fn is_build_output(name: &str) -> bool {
    matches!(name, "target" | "node_modules" | "__pycache__")
}
```

`.gitignore` is never read, and there is no setting for a person to add to the list. Measured on this
repository:

```
$ unluminous-cli --json editor definition relayout
'relayout' has 2 candidate definitions, both "sure":
  crates/unluminous-core/src/layout.rs
  _agent_output/task-1701-git-root-refresh/release-worktree/crates/quill-core/src/layout.rs
```

The second is a gitignored scratch copy of the whole project under its previous name. The same list
feeds `Go to File`, `Find in Files`, the symbol index, references and completion — so a vendored
dependency, a `dist/`, a `.venv/`, a `coverage/` or a second checkout inside the project pollutes all
five, and `editor references relayout` taking **2,811 ms** on this project is partly that.

The comment beside `is_build_output` deliberately leaves `build`, `dist` and `out` out, on the
reasoning that they are real folders in real projects and a search that silently missed a file would
be worse. That reasoning is right and it argues *for* reading `.gitignore` rather than for a longer
hardcoded list: the file says which of them this project means.

**Fix.** Read `.gitignore` (and `.git/info/exclude`) at the project root, honour it in
`FileTree::all_files` while continuing to *show* everything in the explorer, and add an
`editor.exclude` setting for patterns of a person's own. Keep the three names as the fallback for a
folder that is not a repository. **Test:** a fixture project with a `.gitignore`, asserting that
`all_files` excludes what it names and the explorer still lists it.

### 7.4 `editor complete` has no default limit — **unbounded payload**

Measured: `editor complete --stem rel` returns 1,274 completions and 430 KB. `--stem a` returns
**1.28 MB**, roughly 320,000 tokens — more than any model's context, from one keystroke's worth of
stem. `--limit` exists and its documented default is *"All of them when it is left out."*

This is the rule `task-1704` set for the whole surface — *"answer in a payload proportionate to the
question"* — with one command left out of it.

**Fix.** Default `--limit` to 50, ordered as it already orders them, and say in the reply how many
were found. **Test:** the existing proportionate-reply test extended to cover completion.

### 7.5 A lone `\r` is not treated as a line break — **minor**

`printf 'one\rtwo\r'` opens as **1 line**. Classic-Mac line endings are rare enough that this is
correctly a low priority, but it belongs in the same change as §7.1, since that is where the file's
line ending becomes a value the document holds.

### 7.6 A non-UTF-8 file cannot be opened at all

`Document::open` uses `read_to_string`, so anything that is not UTF-8 is refused. Refusing is
defensible — mangling bytes into replacement characters and then writing them back would be worse —
but it is currently invisible: it is not in the README, not in a plugin's `limitations`, and the only
place it is said is a status-bar line. **Fix.** Either state it plainly as a limitation, or detect a
UTF-16 BOM and Latin-1 and open those read-only with the encoding named in the status bar. State it
either way.

### 7.7 Four dead-code warnings in `unluminous-app`

`services/login_shell.rs` has an unused `MARKER`, an unused `ENV` and an unused `parse`, and
`theme/icon.rs` has a fourth. Small, but a clean build is worth having before a release, and the
`login_shell` three suggest a path that was replaced and half-removed.

---

## 8. Code organisation

The crate split is the strongest structural decision in the project and it is what makes §2 possible:
six crates with no user-interface dependency, each with a written rule about what must never be in it,
and `unluminous-cli` depending on none of them so the client stays a small program with no window
behind it. `unluminous-app`'s four folders — `app/`, `components/`, `services/`, `theme/` — are clear
enough that a new file's home is never in doubt.

Two files have outgrown it.

**`app/mod.rs` is 9,996 lines with no tests in it.** It holds the window's state and, by grep, the
open path, the browser command runner, the plugin tab opener, the file-move writer, the git wiring
and the inline-value cache. It has already been split once — `cli.rs`, `actions.rs`, `completion.rs`,
`debug.rs`, `symbols.rs`, `files.rs`, `folding.rs`, `dock.rs`, `git.rs`, `hover_value.rs`,
`plugin_panes.rs` are all `app/` — and the residue is still ten thousand lines. It is the file every
feature touches, which makes it the file every change risks.

**`app/cli.rs` is 7,837 lines.** By design it is one arm per command and that is the right shape, but
159 arms in one file means the database's arms, the browser's arms and the editor's arms are
neighbours for no reason. Splitting it per area — `cli/editor.rs`, `cli/git.rs`, `cli/database.rs` —
keeps `run_cli` as the one place a command becomes a change while making each area readable.

**`tests/screenshots.rs` is 15,513 lines**, which is 88% of the test code in the crate. Same
observation: one file per area of the window, sharing the `builder()` that the device pool depends on.

None of this is urgent and none of it should be done at the same time as §7. It is listed because a
release is the moment the codebase stops being one person's and starts being something other people
read.

### 8.1 The design system, and the one place it is not kept

The palette is closed and the discipline is real: `theme::color` is the whole list, a theme says what
a name *means* and cannot add a forty-first name, and the 22 raw `Color32::from_rgb` calls outside
`theme/` are all conversions of a colour something else already decided — a syntax token, a terminal
palette entry, an epic's colour from the board — rather than hardcoded hex. The one thing missing is
that **none of it is enforced by a test**, unlike almost every other invariant in this project. A
grep-based test over `components/` that fails on a literal hex triple would cost an hour and would
keep the rule after the person who wrote it has stopped watching.

The style guide's own finish condition — *"It has a baseline in `design/components/`, and someone has
opened the image and looked at it"* — is met for 22 components out of 44. Everything since
`task-1756` is in the missing half: the browser view, the database tree and grid and console, the
agent chat pane, the agent-tasks board, the settings dialog, the MCP page, the modal furniture itself.
Those are the newest and least-reviewed surfaces in the product, and they are the ones with no
intended image to be measured against.

---

## 9. What this recommends, as work

Nothing in this document has been applied. `task-1803` asked for an evaluation and a follow-up
ticket, and the follow-up is where the work goes. Suggested shape, in the order of §0:

**A. The seven defects (§7).** Line endings kept and written back, with the byte-comparison round-trip
test. `tab open` answering honestly, with the state-not-reply test applied to every command that can
fail late. `.gitignore` honoured, plus an `editor.exclude` setting. A default limit on `editor
complete`. The `\r` case and the non-UTF-8 statement folded into the first. The four warnings cleared.

**B. Find and Replace (§3.1).** Find in the current file on `Ctrl/Cmd+F`, with match count, next and
previous, case and whole-word. Replace and Replace All beside it, and replace across the results of
`Find in Files`. Both reachable as `editor find` and `editor replace` in the catalogue, both with
screenshot tests, and an `s23-replace` scenario in the study.

**C. Release engineering (§6).** A CI workflow running the suite on both platforms. `signtool` in
the Windows installer once a certificate exists. An update check against the GitHub releases the
tooling already publishes, off by default until a person is asked. `LICENSE-MIT` and `LICENSE-APACHE`
at the root. A CHANGELOG generated from the ticket-prefixed commits. A pass over
`documentation/overview.md`.

**D. Typing cost (§5.2).** Incremental tokenising, exactly as `task-1666` §12 describes it. The
measurement to beat is 90.35 ms and the instrument is `frame_cost`.

**E. The agent gaps (§4.2).** A `chat` and a `database` area in the catalogue, or plugin settings
reachable through `settings`. An `--areas` filter on `mcp serve`. Scenarios for the browser, the
database and the chat pane, and a run of the study against the local model afterwards.

**F. Accessibility (§3.4).** Scoped separately, because it is larger than the rest of this list put
together.

Also worth doing whenever it is convenient: a nightly for the six ignored `agent_board` tests
(§2.2), the grep test for hardcoded colours (§8.1), design baselines for the missing 22 components,
and the splits in §8.

---

## 10. What was deliberately left alone

Per the ticket, this is an evaluation. **No production behaviour was changed, no default was moved,
and no code was edited.** Two windows were launched to reproduce the defects and both were closed.
The fixture files written while measuring are in
`ai-service/_agent_output/task-1803-unluminous-release-eval/` alongside the test logs and the
`frame_cost` output, and the 2 MB file built to measure the typing curve was deleted after it was
measured.
