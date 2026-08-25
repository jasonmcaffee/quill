# Qwen 3.8 27B driving Quill from the command line

**Result: 64 out of 64, five rounds running. 100%.** The bar `task-1661` set was 97%.

The same 64 instructions with the documentation withheld score **3.13%**. That second number is the
one that makes the first mean anything.

---

## What was measured

The model is given two things and nothing else:

1. a short system prompt saying it drives Quill through a tool called `quill-cli`, and that its
   answer must be command lines and nothing else;
2. the whole of `quill-cli/docs/commands.md` — 32,123 characters, about 8,000 tokens.

It is then given one instruction at a time, phrased the way a person would say it. **No instruction
names a command, a flag or a setting.** "Collapse the file tree on the left, I want more room."
"Let more of my desktop show through the window — set it to 40 percent opaque." "Something is stuck
in the terminal — send it a Control-C." A test that says "run `explorer hide`" measures copying.

Every answer is then:

- **parsed** — the command lines are read out of the reply;
- **inspected** — each line is run under `--dry-run`, which reports the command and the arguments it
  would send without sending them, so a task that only asks for information can still be checked;
- **executed** — against a **live Quill window**, for real;
- **checked** — by a predicate over what the window reported about itself afterwards.

A task passes only if all four steps do. Nothing is graded by a model. "Put the cursor on line 4,
column 3" passes because `editor caret` afterwards answers `{"line":4,"column":3}`, not because an
answer looked plausible.

The harness is `run.js`, the instructions are `tasks.js`, the sample project is `project.js`, and
every prompt, every answer and every reason is in
`_agent_output/task-1661-quill-cli/assessment/*.json`.

## The numbers

| Round | Temperature | Documentation | Score |
|---|---|---|---|
| final-3 | 0 | given | **64/64 — 100%** |
| final-4 | 0.6 | given | **64/64 — 100%** |
| final-5 | 0.6 | given | **64/64 — 100%** |
| final-6 | 0.6 | given | **64/64 — 100%** |
| final-7 | 0 | given | **64/64 — 100%** |
| control-no-docs | 0 | **withheld** | 2/64 — 3.13% |

Both temperatures, because a real agent does not run at zero and a result that only holds at zero is
a result about greedy decoding rather than about the tool. Three rounds at the model's own default of
0.6 came back identical to the two at 0.

Median time per instruction: **0.8 seconds**, end to end — the model answering, the command running
against the window, and the window being asked what happened. A whole 64-task round takes just under
a minute.

## The control, and why it is here

A 100% is worth nothing on a test that cannot be failed. So the same 64 instructions were run again
with the reference replaced by the sentence *"There is no documentation available. Work out the
commands yourself."*

It scored **2 out of 64**. The two that survived were `git status` and `terminal hide` — both cases
where the obvious guess happens to be the real command. Everything else was invented:
`quill-cli set-root src`, `quill-cli status set "step three done"`, `quill-cli open README.md`.

So the 97-point gap between 3.13% and 100% is what the documentation is worth. The model is not
recognising Quill from its training data, and it is not guessing a conventional CLI. It is reading.

## How it did, task by task

**It was strongest where the vocabulary was Quill's own.** All nine settings tasks passed every
round, including `appearance.background.opacity` from *"40 percent opaque"* — it converted the
percentage to `0.4` and did not have to be told the range. Using the settings-file names as the CLI's
names, rather than inventing a second vocabulary, plainly paid: there is one spelling of every
setting and it appears in the reference beside what it accepts.

**It handled multi-step work without being told to.** *"Find the file whose name matches 'mdrs' and
open the first thing it finds"* produced three commands — `modal open go-to-file --query mdrs`, then
`modal results`, then `modal accept 0` — and the window ended up on `markdown.rs`. *"Find PORCUPINE
somewhere in the project and take me to it"* did the same through `find-in-files`, and remembered to
wait for the search. Nothing in the system prompt describes a workflow; the recipes section of the
reference does, and it followed them.

**It read the flags carefully.** *"Put 'cargo build' on the terminal prompt but do not run it yet"*
produced `terminal send --no-enter cargo build` — with the flag before the text, which is the one
piece of syntax the reference has to explain. *"That dialog is too small — make it 900 wide and 600
tall"* produced `modal size --width 900 --height 600` and not a guess at positional arguments.

**It picked the escape hatch when asked to.** *"Use the menu entry that toggles the line numbers,
rather than the setting"* produced `action run toggle-line-numbers` rather than
`settings set editor.line_numbers false`, which is the distinction that sentence turns on.

**It never invented a command in a scored round.** Across 448 graded instructions with the
documentation present, every line the model produced parsed and named a command that exists. That is
the single most useful property for an agent, and it is the one the control shows is not free.

## What the two failures were, and what they say

Across the seven scored rounds — 448 graded instructions — there were two failures, both before the
last change. Both were failures of the *tool or its documentation*, not of the model, and both are
fixed:

- **`quill-cli version --json` printed a sentence.** The documentation tells an agent to pass
  `--json` to everything; this one command ignored it. The model did the right thing and the tool
  handed it back something unparseable.
- **"What version of the Quill editor am I running?"** was answered with `quill-cli version`, which
  reports the *command line tool's* version rather than the *editor's*. Fair miss on a genuine
  ambiguity: the reference now says so in that command's own summary, and points at `status`.

That pattern is the assessment's most useful finding. It found three more defects the same way —
a trailing `--json` being swallowed by a text argument, a screenshot captured mid-animation, and
`tab reload` claiming to discard unsaved changes while silently refusing to. **Every failure in this
exercise was mine.** Running a model against a live application is a very effective way to find the
places where a tool does not do what its own documentation says.

## What this does and does not establish

**It establishes** that a 27-billion-parameter model running locally, given nothing but this
reference, can carry out ordinary editor instructions phrased in English, correctly, repeatably, at
both temperatures, across every part of the window: files and tabs, the text and the caret, the
terminal and what is typed into it, the explorer, all five modals, every setting, git and the menus.

**It does not establish** that it can plan. Each instruction is one step or one short obvious
sequence; nothing here asks the model to decide *what* to do, recover from a refusal, or work out a
goal over twenty commands. The scores say the interface is legible, not that the model is capable.

**It does not establish** anything about a smaller model. One local model was measured, and the
control shows the score is carried by the documentation rather than by the weights — which is a
reason to expect a smaller model to do better here than on most tasks, not a reason to assume it.

## What made the difference

Three things, in the order they mattered:

1. **One reference, generated from the catalogue.** Every command has a section with its real usage
   line, and a test fails while any command is missing or any usage line is stale. The model is never
   reading a description of a command that has since changed.
2. **Every example is tested.** A test parses every example in the catalogue and checks it runs the
   command it is filed under. The examples are what the model copies, and an example that does not
   work is worse than no example.
3. **Errors that say what to do instead.** `action run open-file` does not fail with "not
   supported"; it says *"opens the platform's file chooser, which nobody can click from a script. Use
   `quill-cli tab open <path>` instead."* Every refusal names its alternative, which is what turns a
   dead end into a next step.

## Reproducing it

```sh
# a Quill on the sample project, and nothing else running
node quill-cli/agent-assessment/project.js _agent_output/task-1661-quill-cli/assessment-project
quill-cli launch _agent_output/task-1661-quill-cli/assessment-project

# the model: Qwen3.8-27B-IQ4_XS on one card, llama.cpp, q8_0 KV, MTP-3, 40k window
llama-server.exe -m J:\llm-models\qwen-3.8\Qwen3.8-27B-IQ4_XS.gguf -ngl 9999 --host 127.0.0.1 \
  --port 8087 -dev cuda0 -c 40960 -fa on -ctk q8_0 -ctv q8_0 --spec-type draft-mtp \
  --spec-draft-n-max 3 --jinja --chat-template-file J:\llm-models\qwen-3.8\chat-template-fixed-3.8.jinja

node quill-cli/agent-assessment/run.js --label mine --temperature 0
node quill-cli/agent-assessment/run.js --label mine-control --temperature 0 --no-docs
```

The runner exits 0 at or above 97% and 1 below it, so it can be a gate rather than a report.
