# task-1695 — what happened when an agent drove Unluminate

A supervised observation of **Qwen 3.8 27B** (`Qwen3.8-27B-IQ4_XS`, llama.cpp, 96k window) driving a
real Unluminate window through the MCP server, across 23 scenarios covering every area the server offers.

Each scenario is phrased the way a person would say it — *"I want main.rs on the left and shapes.rs on
the right"* — rather than as a command. Unluminate is reset to a known state between scenarios, and each
one's result is checked by reading Unluminate's own state back through `unluminate-cli` rather than by believing
what the agent said it did. The harness is `tools/agent-study/`; transcripts land in `_agent_output/agent-study/sessions/`, one `.md` per scenario, with every tool
call, argument and refusal.

## The numbers

| | |
|---|---|
| Scenarios | 23 |
| Tool calls | 126 |
| …through Unluminate's MCP | 96 |
| …through the agent's own `bash`/`grep`/`read`/`edit` | **30 (24%)** |
| Refused calls | 9 |
| Scenarios where the agent used its own tools instead of Unluminate's | **13 of 23** |

## The headline: reachable is not the same as reached

Asked cold what it could do with Unluminate, the agent answered accurately and without a single call — the
grouped tool descriptions are good enough to be read as documentation. It named git, search,
go-to-definition and the file tree among its capabilities.

It then did not use any of them.

- **git** — ran `bash git status` and `bash git diff`. Unluminate has `git status` and the whole Git menu.
- **find every use of a name** — ran its own `grep`. Unluminate has `editor references`, which classifies
  each hit as code, comment or string, and reads open tabs as they stand rather than as the disk has
  them.
- **go to a definition** — ran `grep '(struct|class|type|interface|typealias)\s+Rect'`. Unluminate has
  `editor definition`.
- **make a folder and a file** — ran `bash mkdir -p tests && touch tests/smoke.rs`, then
  `bash find .` to show the tree. Unluminate has `explorer new-folder`, `new-file` and `tree`.
- **rename a symbol across the project** — grepped, read three files, and ran three
  `edit --replaceAll`. Unluminate has `editor rename`, which is one undo step per file, leaves comments
  and strings alone unless asked, and knows which files are open.

The last one is the one that matters. The agent's replace-all rewrote the mermaid diagram inside
`README.md`, which Unluminate's rename would have left alone by default. It wrote three files behind the
back of a window that had two of them open. And it was not one undo step. **Unluminate's most valuable
refactoring feature was bypassed and the result was worse than Unluminate's would have been.**

One cause is concrete and fixable: **`editor definition` takes only a position — an offset, or a line
and column — and never a name**, while `editor references` and `editor rename --name` both accept
one. An agent that knows a *name* and wants its definition has no way in, so it greps to find an
occurrence first, and having grepped it no longer needs Unluminate.

The other cause is that nothing in the tool descriptions says what Unluminate knows that `grep` does not.

## The worst single moment: the debugger was driven correctly and the answer was invented

s08 asked for a breakpoint on the line that prints the total, a debug session, and the value of
`total` when it stopped. The agent set the breakpoint, started the session, stepped to the right line
— all through Unluminate, all correctly — and then answered:

> `total` is **35.5** (3×4 + 5×2 + 1.5×9 = 12 + 10 + 13.5).

It did the arithmetic on the source. It never called `debug variables`.

The answer was already in the payload it had been handed. Measured on the same session by hand:

```
variables: [ ... { "name": "total", "type": "double", "value": "35.5" } ... ]
frames count: 19 | subtle frames: 14 | payload bytes: 3214
```

`total = 35.5` was there, behind nineteen stack frames, fourteen of which Unluminate itself marks
`subtle: true` because it already knows they are Rust runtime noise —
`std::panicking::catch_unwind`, `core::ops::function::impls::impl$2::call_once` and so on. Every
debug verb returns the same full envelope: `start`, `step-over`, `status` and even `variables` all
ship all nineteen.

It came out right by luck. Had the program had a bug, the agent would have reported the value the
source *implies* rather than the value the program *holds* — which is the entire reason a debugger
exists, and precisely the failure task-1692 was filed about.

## Defects, each with the evidence behind it

**1. The git root is resolved once at launch and never re-checked.** With a project that had become
its own repository after the window opened, Unluminate reported:

```
root: C:/jason/dev/unluminate   branch: main   changed: 0
```

…while the project itself was `scratch-project`, on `master`, with nine changes. Unluminate was answering
about an **ancestor repository**, confidently, with a plausible branch name and a plausible zero.
Restarting the window fixed it, which is what identifies it as a cache rather than a resolution bug.
`git action commit` in that state would have committed to the wrong repository. Unluminate already has the
rule this needs — task-1691's "the disk-owned side is re-checked at the moment of use".

**2. `undo` restores the text but leaves the document modified.** Measured directly:

```
before:            modified = false   chars = 486
after insert+undo: modified = true    chars = 486
```

The bytes are identical to the disk and the tab claims unsaved changes. The consequences compound:
`tab reload` then refuses ("has unsaved changes, so it was not reloaded"), the tab draws a dirty
marker, and `close_tab` would *write* the file. In s21 the agent, having only wanted to look at a
completion list, was cornered into **saving a file it never meant to change**.

**3. `editor complete` cannot be asked hypothetically.** It completes the word being typed at the
caret, so "what would I be offered if I typed `ar`" requires actually typing `ar` into the person's
file and undoing it — which is what triggered defect 2 above. There is no `--stem`.

**4. `terminal send` and `terminal read` cannot target a tab.** One area, four conventions:

| verb | how a tab is named |
|---|---|
| `rename`, `move` | `--tab <index>` |
| `close`, `select` | a positional |
| **`send`, `read`** | **cannot — always "the tab that is showing"** |

The two verbs where targeting matters most are the two that lack it. The agent had just made and
named tab 1, wrote `{"text":"cargo check","tab":1}`, and was refused.

**5. The agent's first guess at a verb is wrong in a predictable way.** `editor open` was tried in
three separate scenarios — the single most common mistake in the study — because an agent thinks the
thing that opens a file is "the editor". It is `tab open`. `editor reload` was tried too; it is
`tab reload`. The refusals are excellent and the agent self-corrected every time, but it is a wasted
round trip that will happen to every agent, for ever.

**6. Argument names must be kebab-case and nothing says so.** Three refusals: `waitFor`, `wait_for`,
`fromLine`/`toLine`. The MCP schema declares `arguments` as `additionalProperties: true` with no
property names at all, so the only place the real spelling exists is prose in the description. A
model writing JSON reaches for camelCase by default. Unluminate already decided that "a name written with
dashes is the same name"; this is that rule not going far enough.

**7. Replies are not proportionate to their answers.**

- Every mutating `fold` command returns the entire region list. Nine regions here to say "9 of 9
  collapsed"; on `app/mod.rs`, which CLAUDE.md records as having 1,276 blocks, it would be 1,276
  objects.
- `status` cannot be asked for one section, so "how many panes are open" costs the whole settings
  dump — roughly 3,000 tokens. The agent called it in five separate scenarios.
- `action list` is 96 rows with no way to ask for one menu. Asked "what's on the View menu", the
  agent got all 96 and then described the View menu from memory rather than from the reply.
- `settings list` runs a long value straight into the description column with no separator:
  `debug.lldb  C:\...\codelldb.exeWhere the LLDB adapter lives`. There is no way to tell where the
  value ends.

**8. `fold expand --line N` opens one nesting level.** Asked to "open `total_area` back up so I can
read it", the agent expanded line 7 and the function was still unreadable — the `for` at line 9 and
the `if` at line 10 were still collapsed. It took three calls, found by trial. There is
`--all` (the whole file) and `--line` (one region) and nothing in between; IntelliJ's
expand-recursively exists for exactly this.

**9. There is no way to ask what is on the screen in words.** The only answer to "did that land" is
`window screenshot`, which returns a PNG. This model has no vision, so it took four screenshots it
could not read, said so twice, and fell back to `status`. A text description of the window — what a
person would see — is the missing verb.

## What worked, and worked well

Worth recording, because it is most of the surface and it should not be disturbed.

- **The grouped tool descriptions are genuinely good documentation.** The cold-open scenario produced
  an accurate capability list with zero calls.
- **Refusal messages are excellent.** Every one named what the command *does* take. The agent
  self-corrected from all nine without further help. This is the single strongest thing about the
  surface.
- **`modal`** — `list` → `open` → `results` → `accept` drove Go to File cleanly, first try.
- **`editor complete`** returned ten candidates with a kind and a source for each. No generic agent
  tool has anything like it.
- **`run add` / `select` / `start` / `output --wait-for`** read a program's real output back.
- **`terminal new` / `rename` / `send` / `read --wait-for`** worked once the argument spelling was
  right, and the agent correctly read a compiler warning out of the grid.
- **`editor view preview` / `editor preview`** returned the rendered Markdown including the
  box-drawn table.
- **`debug breakpoint add` / `start --wait-for-pause` / `step-over`** all did exactly what they
  should. The debugger is not broken; only its reply is.
- **The MCP argument shape is easier than the CLI's.** `{"action":"add","path":"src/main.rs",
  "line":37}` works over MCP where the CLI needs positionals in order. That was a good decision.

## One thing to notice about the state files

In s07 the agent read `.unluminate/run-configurations.conf` directly — Unluminate's own state file, whose
header says it is "safe to edit by hand" — to work out what run configurations were. It had
`run list` available. An agent that reads those files will eventually write them, behind a running
window that owns them in memory.

## The tickets this produced

| | |
|---|---|
| `task-1699` | Make Unluminate's own answer the one an agent reaches for — `editor definition [name]`, and descriptions that say what Unluminate knows that `grep` does not |
| `task-1700` | Debug replies bury the answer under runtime stack frames |
| `task-1701` | The git root is resolved once at launch and never re-checked |
| `task-1702` | Undo leaves a document marked modified, and completion forces a mutation |
| `task-1703` | Make the agent's first guess work — verb and argument-name aliases |
| `task-1704` | Make replies proportionate to what was asked |
| `task-1705` | `terminal send` and `read` cannot target a tab, and the area has four conventions |
| `task-1707` | `fold expand` opens one nesting level, so there is no way to open a whole function |

Finding 9 — no way to ask what is on the screen in words — was raised and **deliberately not
taken**. It is left in the findings above because it is what was observed, not because it is
work that is waiting to be done.

## Running it again

```sh
node tools/agent-study/make-sample-project.mjs
unluminate-cli launch _agent_output/agent-study/scratch-project --no-wait
node tools/agent-study/run-all.mjs
node tools/agent-study/grade.mjs
```

`tools/agent-study/README.md` says what has to be standing up first. The number to watch is the share
of tool calls that went round Unluminate rather than through it: **24% when this was written.**
