# task-1704 — make the reply proportionate to what was asked

Four commands in the catalogue answer a small question with a large object, and one of them answers
with text that cannot be parsed. Every byte is paid in an agent's context on every call, and the
task-1695 study measured what that costs: `status` called in five scenarios with the whole settings
dump in three of them, `action list` returned 96 rows and the agent then **described the View menu
from memory** because the one menu it wanted was buried in the rest, and `fold collapse --all` on a
file with 1,276 foldable blocks would return 1,276 objects to say "1276 of 1276 collapsed".

## 1. What the study found, with the numbers

From `_agent_output/agent-study/sessions/` (the task-1695 run, kept in `_agent_output/agent-study/`):

| Scenario | Call | Bytes in the reply | What was asked |
|---|---|---|---|
| s04-fold | `fold collapse --all` | 1,162 | "fold up all the functions" |
| s04-fold | `fold expand --line 7` (×3 more) | 1,163 each | "open total_area back up" |
| s05-split | `status` | 4,872 | "how many panes are open" |
| s17-screenshot | `status` | 4,918 | which window is this |
| s19-actions | `action list` | 4,793 | "what's on the View menu" |
| s20-project | `status` | 4,918 | "which folder is this showing" |

The fold reply is a sentence — "9 of 9 blocks collapsed" — plus the complete region list, and the
sentence already says everything the caller asked for. The `status` reply is the whole window:
editor, tabs, panes, panels, explorer, terminal, modal, **every setting with its help text**, and
git, for a question that wants one of them. `action list` is 96 rows with no way to ask for one
menu. And `settings list` prints `name`, value and help as three columns where the value column is
16 characters wide and the value may be a path:

```
debug.lldb   C:\Users\jason\AppData\Local\Quill\adapters\codelldb\extension\adapter\codelldb.exeWhere the LLDB adapter lives, for Rust and native code.
```

There is no separator between the value and the help, so a reader — human or model — cannot tell
where one ends.

## 2. What other tools do about this

The pattern is the same everywhere a CLI is built to be read by a program:

- **`gh pr list --json <fields>`** — the caller names the fields it wants and gets only those. The
  default table is for a person; the JSON is field-selected for the caller.
- **`git status --porcelain`** — a stable, machine-shaped answer to the same question a person gets
  in prose. The format is the contract; the content is exactly the answer.
- **`kubectl get -o jsonpath` / `--jq`** — projection is the caller's right, not the server's
  guess.

Quill's catalogue is already shaped for this: every command is one row with named arguments and
flags, the client parses against it, and the MCP tools are generated from it. So the fix is not a
new mechanism, it is **four flags in the catalogue and the window honouring them**, and the MCP
tools pick them up the day they are added, with no tool written by hand.

The rule this keeps: **the reply is proportionate to the question, and the question is what the
caller said.** A flag that narrows the answer is a value the catalogue names, so a misspelled one
is refused by the existing `unknown_arguments` refusal rather than dropped, and the full answer
stays available by leaving the flag out. Nothing is removed from any reply; nothing a caller could
ask for today becomes unaskable.

## 3. The four changes

### 3.1 `fold` mutations answer with a summary; the list is opt-in

`fold toggle`, `fold collapse`, `fold expand` and `fold others` all answer through `fold_answer`,
which builds the sentence and the region list. The sentence — "9 of 9 blocks collapsed" — is the
answer; the list is what a caller needs when it wants to know *which* blocks.

- A new switch, `--regions`, on each of the four mutating verbs. Without it the result is
  `{"collapsed": n, "total": m}` — two numbers. With it, the regions come along beside them.
- `fold list` is unchanged: it is the command whose job is the list, and it keeps returning it.
- The sentence is unchanged in both cases, because it is the summary either way.

### 3.2 `status --section`

`status` answers with one object holding every part of the window. The parts are already separate
keys in that object, so a section is a filter over the keys the value already has:

| Section | Keys it carries |
|---|---|
| `editor` | `editor` |
| `panes` | `panes` |
| `tabs` | `tabs`, `activeTab` |
| `panels` | `panels` |
| `explorer` | `explorer` |
| `terminal` | `terminal` |
| `modal` | `modal` |
| `settings` | `settings` |
| `git` | `git` |
| `window` | `window`, `version`, `buildDate`, `pid`, `port` |
| `project` | `project` |
| `message` | `message` |

- `--section` takes a name, and several, comma-separated, the way `editor references --include
  comments,strings` already takes its list. Case does not matter: an agent writes `view` and the
  menu is `View`, so the comparison is the lowercase one.
- A name that is not a section is a usage refusal that names the sections, for the reason
  `unknown_arguments` exists: a misspelled name is a question that was not asked.
- Left out, the answer is exactly what it is today. The sentence is unchanged: it is one line, it
  is the command's identity, and it is the same whether or not a section was asked for.
- The filter is applied to the value, not to the sentence, and it is applied at the one place the
  value is built, so the answer after the git worker settles is filtered the same way.

### 3.3 `action list --menu`

The list is built by walking the real menus, and every row already carries the menu it is on.
`--menu` filters that list by the menu name, several allowed, comma-separated, case-insensitive —
so `--menu view` and `--menu View` and `--menu view,edit` all work, and a submenu name such as
`Highlight` names its own rows, because the walk records the submenu's name rather than the menu
it sits under.

- A name that matches no menu is a not-found refusal that lists the menus there are.
- The sentence counts what is being returned: "18 entries on the View menu" rather than "96 menu
  entries".
- Left out, the answer is exactly what it is today.

### 3.4 `settings list` keeps its columns apart

The row is `name`, value, help. The value was padded to 16 characters and the help started where
the padding ended, which is where the value started when it was longer than 16. The value column is
now padded to the **longest value there is**, and the help starts two spaces after it, so the
columns stay aligned and the value and the help are apart whatever the value is. The JSON answer is
already one object per setting with the three fields named, so this is the text rows alone.

## 4. What this is not

- **Not a general projection language.** `--json` already hands the caller the whole object, and a
  caller that wants one field out of it has `jq`. What is missing is the four questions the study
  actually asked, and those get a flag each rather than a query language the catalogue would have
  to describe.
- **Not a change to what a command can answer.** Every byte that left a reply before still leaves
  it, by leaving the flag out. The default of every new flag is the old answer.
- **Not a change to `fold list`, `settings get` or any command that already answers one thing.**
  `settings get` was the answer to "what is one setting" all along; the study's agent found it, it
  just cost a round trip to discover.

## 5. How it is verified

1. The catalogue's own tests: every example parses, every command is documented with its current
   usage line, and the MCP tools are generated from the list — so the new flags are on the tools
   the day they are in the catalogue, and `every_command_is_offered_as_a_tool_in_both_shapes` says
   so.
2. Window tests over the real channel, in `tests/screenshots.rs` beside the ones that already drive
   `status` and `action list`: a fold change answers with two numbers and no list, and with the
   list when `--regions` is given; `status --section panes` carries `panes` and nothing else;
   `status --section editor,git` carries exactly those two; a section that is not a section is
   refused; `action list --menu view` returns only View rows and a menu that is not a menu is
   refused; `settings list` keeps a long value and its help apart.
3. `quill-cli mcp tools --count` before and after: the tool count is unchanged and the byte
   figures say what the new flags cost in the agent's context.
4. The study harness, `tools/agent-study/`, re-run on the four scenarios this is about — s04, s05,
   s19 and s20 — against the rebuilt window, with the reply sizes read out of the session files and
   compared with the table in §1.

## 6. Live verification, with the numbers

Driven against a rebuilt window (`target/release/quill-cli.exe` → the window on
`_agent_output/agent-study/scratch-project`), the four replies answer proportionately when asked to,
and answer exactly as before when not:

| Reply | Full (flag left out) | Proportionate |
|---|---|---|
| `status` | 7,704 B | 507 B `--section panes` · 283 B `--section project` |
| `fold collapse --all` | 1,406 B (with `--regions`) | 143 B summary, the study's agent paid 1,162 for the default |
| `action list` | 27,751 B | 4,813 B `--menu view` (the 19 rows the study's agent wanted) |
| `settings list` | — | a 76-char `debug.lldb` path now sits two columns clear of its help |

The refusals answer the way the rule says: `status --section purple` → "`purple` is not a section of
`status`. It is one of: editor, tabs, panes, …"; `action list --menu purple` → "There is no menu
called `purple`. The menus are: Quill, File, …". Several sections and case are honoured:
`--section editor,git` carries exactly those two.

`quill-cli mcp tools --count`: the tool count is unchanged (22 grouped / 148 every); the new flags
cost 61,516 → 62,090 B grouped and 145,735 → 147,416 B every — the flags are on the tools, and the
default of each is the old full reply.

## 7. Study-harness status: blocked by the model, not the change

The harness was re-run on s04, s05, s19 and s20 against the rebuilt window, and every scenario came
back with **zero tool calls**: the local model answered every turn with a 500 —
`the current context does not logits computation. skipping`. That is the model server, not Quill:
port 8087 (the port `qwen38-study/qwen38-27b` is configured for) is serving the **nomic embeddings**
model, which cannot do logits, and the chat model on 8080 is not answering new requests. Re-running
the four scenarios needs the study model (`Qwen3.8-27B-IQ4_XS.gguf`, 15.7 GB) loaded on 8087, which
the machine's two 5090s cannot take with the current models resident (8–10 GB free each).

One cost to note: the re-run wrote over the task-1695 session files in
`_agent_output/agent-study/sessions/` (s04, s05, s19, s20) with the empty zero-tool-call versions,
because it used the default output folder rather than a fresh `STUDY_OUT`. The §1 byte figures were
read out of those files before the overwrite and are unchanged here.
