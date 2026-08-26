# Driving Quill from the command line

`quill-cli` drives a **running** Quill window. Everything the menus, the keyboard and the mouse can
ask for is a command, and every command answers with either a sentence or, with `--json`, a value a
program can read.

This document is written to be handed to an AI agent whole. If you are an agent, read the next two
sections and then work from the reference at the bottom.

**If your client speaks the Model Context Protocol, you do not need this document at all.** Quill has
an MCP server, generated from the same list of commands, so the tools are the commands and they
cannot fall behind. `quill-cli mcp install claude`, or the buttons in `Settings -> Tools -> MCP`.
[mcp.md](mcp.md) is the whole of it.

## The four things to know first

1. **Always pass `--json`.** Without it you get a sentence meant for a person; with it you get
   `{"ok":true,"command":"…","message":"…","result":{…}}`, and `ok` tells you whether it worked. A
   failure is `{"ok":false,"command":"…","error":{"code":"…","message":"…"}}`.
2. **`quill-cli commands --json` prints every command as data** — the areas, the arguments, the
   flags, the examples. It needs no running Quill. It is the same list this document is generated
   from, so it can never be out of date.
3. **A command is `quill-cli <area> <verb>`** — the thing first, then what to do to it:
   `tab open`, `terminal send`, `modal results`, `settings set`. Six commands have no area:
   `status`, `instances`, `launch`, `quit`, `commands`, `version`.
4. **`quill-cli status --json` tells you where you are** — the project, the tabs, the panes, the
   terminal, the modal that is open, the settings and git, in one answer. Start there.

## Getting a Quill to talk to

```sh
quill-cli instances --json          # the Quill windows that are running
quill-cli launch C:\jason\dev\quill # start one, and wait until it answers
```

`launch` returns only once the new window is answering, so the next command in a script cannot be
too early.

With one Quill running, every command goes to it. With several, say which:
`--instance <pid>`, `--instance <port>`, or `--instance <part of the project's path>`. With several
running and none named, the command is refused and lists them rather than guessing.

## Paths

**A relative path is relative to the project folder** — not to wherever you ran the command. That is
one rule for `tab open`, `explorer expand`, `window screenshot`, `tab save-as` and every other path
argument. Give a path in full to reach something outside the project. Every reply says which
absolute path it used, so there is never any doubt.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | It worked. |
| 1 | Quill refused it: no such file, no such tab, nothing to undo. |
| 2 | The command line was wrong: no such command, no such flag, a missing argument. |
| 3 | No Quill is running, or the one named could not be reached. |
| 4 | Several Quills are running and none was named with `--instance`. |
| 5 | Quill was reached but did not answer in time. |

`2` is your mistake, `1` is Quill's answer, and `3`, `4` and `5` are about the connection.

The `error.code` in a JSON reply is the same thing in words: `not-found`, `not-applicable`, `usage`,
`unknown-command`, `refused`, `failed`, `timed-out`, `not-running`, `several-instances`.

## Flags that work on every command

| Flag | What it does |
|---|---|
| `--json` | Print the whole reply as JSON. Always pass this from a program. |
| `--instance <pid\|port\|path>` | Which Quill, when several are running. |
| `--quiet` | Print nothing when it worked. The exit code still says whether it did. |
| `--timeout <milliseconds>` | How long to wait for an answer. 15000 by default. |
| `--dry-run`, `-n` | Print the command and arguments that *would* be sent, and send nothing. Needs no running Quill, so it checks a script before there is a window. |
| `--no-color` | Never colour the output. `NO_COLOR` in the environment does the same. |
| `--help` | Help for the command, or for the whole CLI. |
| `--version`, `-V` | The version. |

The single-letter forms `-h`, `-n`, `-q` and `-V` are read **only before the command name**
(`quill-cli -n tab open x`). After it, only the long `--` forms are Quill's — which is what keeps a
shell command's own `-n` safe in `quill-cli terminal send git log -n 5`.

## Quoting, and the commands that take the rest of the line

Some arguments swallow everything after them, so a shell command needs no quotes:

```sh
quill-cli terminal send git log --oneline -n 5
```

`git log --oneline -n 5` is all sent to the shell. One rule decides what is text and what is not:

> **Quill's own flags are recognised anywhere on the line. Anything else is text.**

So `--oneline` is text, because Quill has no such flag; `--json` is Quill's wherever it sits, so this
does what it looks like:

```sh
quill-cli settings set appearance.font.size 20 --json
quill-cli terminal send --no-enter cd ..
quill-cli terminal send git status --no-enter      # the same thing; --no-enter is Quill's
```

To send text that *is* one of Quill's flags, put `--` in front of it — after that nothing is a flag:

```sh
quill-cli terminal send -- curl --json https://example.com
```

The arguments that take the rest of the line are marked in the reference below. They are
`terminal send`'s text, `editor set-text`, `editor insert`, `explorer filter`, `modal type`,
`window message` and `settings set`'s value.

## Recipes

**Open a file and look at it.**

```sh
quill-cli tab open README.md --json
quill-cli editor text --from-line 1 --to-line 20 --json
```

**Find a file without knowing where it is.** `Go to File` matches a subsequence, so `mdrs` finds
`markdown.rs`.

```sh
quill-cli modal open go-to-file --query mdrs --json
quill-cli modal results --limit 5 --json
quill-cli modal accept 0 --json
```

**Search the project's text and jump to a match.** The search runs on a thread, so wait for it.

```sh
quill-cli modal open find-in-files --query "fn main" --json
quill-cli modal results --wait 5000 --limit 10 --json
quill-cli modal accept 0 --json
```

**Run a shell command and read what it said.**

```sh
quill-cli terminal show --json
quill-cli terminal send cargo --version
quill-cli terminal read --wait-for cargo --timeout 15000 --json
```

**Edit a file and save it.**

```sh
quill-cli tab open notes.md --json
quill-cli editor caret --line 1 --column 1 --json
quill-cli editor insert "# Notes\n\n" --json
quill-cli tab save --json
```

**Change how it looks, then look at it.**

```sh
quill-cli settings set appearance.font.size 20 --json
quill-cli settings set appearance.background.opacity 0.6 --json
quill-cli window screenshot _agent_output/quill.png --json
```

**Mark the passages a piece of work is about.** A highlight is a colour behind a range of text. It
stays until it is cleared, it comes back next time the project is opened, and it moves with the text
as the file is edited. The file does not have to be open.

```sh
quill-cli highlight add src/main.rs --from-line 40 --to-line 58 --color blue --json
quill-cli highlight add src/main.rs --text "unwrap()" --color pink --json
quill-cli highlight list --all --json
quill-cli highlight clear src/main.rs --json
```

**Mark many passages across many files in one call.** This is the shape to use when the places are
already worked out — one request rather than twenty, and none of the files is opened.

```sh
cat > marks.json <<'JSON'
[
  { "path": "src/main.rs",     "fromLine": 40, "toLine": 58, "color": "blue" },
  { "path": "src/parser.rs",   "fromLine": 12, "toLine": 19, "color": "yellow" },
  { "path": "docs/design.md",  "fromLine": 3,  "toLine": 3,  "color": "green" }
]
JSON
quill-cli highlight apply --from-file marks.json --replace --json
quill-cli window screenshot _agent_output/marked.png --json
```

`--replace` clears every mark in the project first, so what is applied is all there is — which is
what to use when the marks are the current state of a piece of work rather than something to add to.
A row that cannot be applied is reported against its number and the rest of the list still goes in.

**Find where a name is defined, and everywhere it is used.** Quill reads a project's definitions
from the same tokeniser that colours it, so the answer is instant and it is honest about what it
does not know: where two files define the same name, both are listed and neither is guessed at.
A file whose language has not said what a definition looks like has none — `.txt`, Markdown and
CSS among them — and the commands say so rather than answering emptily.

```sh
quill-cli editor caret --line 42 --column 9 --json
quill-cli editor definition --json
quill-cli editor definition --open --json
quill-cli editor references --json
quill-cli editor navigate-back --json
```

Each reference says whether it is code or a word inside a comment or a string. `--code-only` leaves
the textual ones out, which is usually what a script wants.

**Rename a name everywhere it is used.** Print the change set first; `--apply` makes it. An open tab
is edited as a document — one undo step, and it is left with unsaved changes rather than being
written behind your back — and a closed file is checked to still hold the old name before it is
rewritten. A file that changed since the search is skipped whole and reported by name.

```sh
quill-cli editor caret --line 42 --column 9 --json
quill-cli editor rename open_the_result --json
quill-cli editor rename open_the_result --apply --json
quill-cli tab save --json
```

The default scope follows what the name resolves to: a variable, a parameter or a name with no known
definition changes in **this file**, and a function, type, constant or module changes across the
**project**. `--scope file` and `--scope project` say so outright, and `--include comments,strings`
adds the textual matches, which are left alone by default.

**Finish a name that is half typed.** The editor offers the names the word under the caret could
become — this file's own words and definitions, the other open tabs' definitions, the project's, and
the language's own keywords — ranked best first. `--choose` applies one of them to the word being
typed, which is exactly what pressing `Enter` on that row in the popup does.

```sh
quill-cli editor caret --line 42 --column 9 --json
quill-cli editor complete --json
quill-cli editor complete --choose open_the_match --json
```

Matching is a subsequence and it ignores case, so `lyt` finds `layout` and `pt` finds `paint_text`.
The row equal to what has already been typed is never offered, because taking it would change
nothing. A file no plugin claims — Markdown, plain text, a picture — has no completions and says so.

**Do something with no command of its own.** Every menu entry has a name:

```sh
quill-cli action list --json
quill-cli action run toggle-line-numbers --json
quill-cli action run highlight-yellow --json
quill-cli action run clear-highlight --json
quill-cli action run go-to-definition --json
quill-cli action run find-references --json
```

## Two things the CLI will not do

**It will not open a file chooser.** `open-file`, `open-folder` and `save-as` are entries on the File
menu that ask the platform for a window somebody has to click in, which from a script is a window
nobody is looking at. `action run` refuses all three and names the command that takes the path
instead: `tab open`, `project open`, `tab save-as`.

**It will not drive the commit panel's message and file list.** `git action commit` opens the panel;
committing is done in it, or with git in a terminal.

## Adding a command

Quill's rule is that a new feature comes with a command and with documentation. Both are enforced
rather than remembered:

- A command is a row in `quill-cli/src/catalogue.rs` and an arm in
  `crates/quill-app/src/app/cli.rs`. The client parses against the catalogue, so a command in it is
  a command the CLI accepts.
- A **menu entry** needs nothing at all: `action list` walks the real menus, and a test in
  `crates/quill-app/src/app/action_names.rs` fails if a menu entry has no name.
- The reference below must then be regenerated:
  `cargo run -p quill-cli --example reference`. A test in `quill-cli/src/documentation.rs` fails
  while a command has no section here, while a section's usage line is out of date, or while a
  section describes a command that no longer exists.

---

The rest of this document is generated from the catalogue. Do not edit it by hand; run
`cargo run -p quill-cli --example reference` instead.

<!-- begin generated reference -->

## Commands with no area

Six commands are typed on their own, because they are about the CLI or about a whole Quill rather than about one part of a window.

### status

```
quill-cli status
```

Everything about the window in one answer: its version and build date, the project, the tabs, the panes, the terminal, the modal that is open, the settings and git.

```sh
quill-cli status --json
```

### instances

```
quill-cli instances
```

The Quill windows that are running, with the port and the project of each. Answered without talking to any of them.

```sh
quill-cli instances --json
```

Answered by the CLI itself; no Quill needs to be running.

### launch

```
quill-cli launch [folder] [--timeout <milliseconds>] [--no-wait]
```

Start another Quill on a folder and wait until it answers.

- `folder` (optional) — The project to open. The current folder when it is left out.

- `--timeout <milliseconds>` — How long to wait for the new window to answer. 20000 by default.
- `--no-wait` — Return as soon as the process starts, without waiting for it to answer.

```sh
quill-cli launch C:\jason\dev\quill
quill-cli launch . --timeout 40000
```

Answered by the CLI itself; no Quill needs to be running.

### quit

```
quill-cli quit
```

Close the window. Its settings and what it had open are written down first, as they are when it is closed by hand.

```sh
quill-cli quit
```

### commands

```
quill-cli commands [name]
```

Every command this CLI has, as data: the areas, the arguments, the flags and the examples. This is what to read first when a program or an agent is driving Quill.

- `name` (optional) — One command, such as `terminal send`, instead of all of them.

```sh
quill-cli commands --json
quill-cli commands "modal open" --json
```

Answered by the CLI itself; no Quill needs to be running.

### version

```
quill-cli version
```

What version this command line tool is. The version and build date of the Quill editor it is talking to are in `status`, and `modal open about` shows them in the window.

```sh
quill-cli version
```

Answered by the CLI itself; no Quill needs to be running.

## window — the window itself

`window screenshot` is how to see what a command did. The picture is of the real window, so it is evidence rather than a description.

### window screenshot

```
quill-cli window screenshot <file> [--timeout <milliseconds>]
```

Write what the window is showing to a PNG file. The picture is of the real window, so it is how what a command did can be looked at.

- `file` — Where to write the PNG. A folder that is not there is made.

- `--timeout <milliseconds>` — How long to wait for the picture. 5000 by default.

```sh
quill-cli window screenshot _agent_output/after.png
```

### window focus

```
quill-cli window focus
```

Bring the window to the front and give it the keyboard.

```sh
quill-cli window focus
```

### window size

```
quill-cli window size [--width <points>] [--height <points>]
```

Read how large the window is, or set it. A fixed size is what makes two screenshots comparable.

- `--width <points>` — How wide to make it.
- `--height <points>` — How tall to make it.

```sh
quill-cli window size
quill-cli window size --width 1100 --height 720
```

### window position

```
quill-cli window position [--x <points>] [--y <points>]
```

Read where the window is on the screen, or move it.

- `--x <points>` — How far from the left of the screen.
- `--y <points>` — How far from the top of the screen.

```sh
quill-cli window position --x 40 --y 40
```

### window message

```
quill-cli window message [text]
```

Read the line the status bar is showing, or put a line of your own there.

- `text` (optional) — What to show. The line is cleared when this is left out. Everything after it on the line belongs to it.

```sh
quill-cli window message
quill-cli window message Ready for the next step
```

## tab — the files that are open

A tab holds a file. A relative path is resolved against the project folder, and every reply says which absolute path it used.

### tab open

```
quill-cli tab open <path> [--permanent]
```

Open a file in a tab and show it. A picture opens as a picture; anything else opens as text.

- `path` — The file. A relative path is resolved against the project folder.

- `--permanent` — Open it as a tab of its own rather than reusing the tab a single click reuses.

```sh
quill-cli tab open README.md
quill-cli tab open design/style-guide.md --permanent
```

### tab list

```
quill-cli tab list
```

The tabs that are open, in order, with the path, the name and whether each has unsaved changes.

```sh
quill-cli tab list --json
```

### tab show

```
quill-cli tab show <tab>
```

Show a tab that is already open.

- `tab` — Its number counting from 0, or its name, or its path.

```sh
quill-cli tab show 2
quill-cli tab show README.md
```

### tab close

```
quill-cli tab close [tab] [--discard]
```

Close a tab. A tab with unsaved changes is written first, which is what closing one by hand does. Closing the last one leaves an empty untitled tab rather than no tab at all.

- `tab` (optional) — Its number, name or path. The tab that is showing when it is left out.

- `--discard` — Close it without writing what was typed into it.

```sh
quill-cli tab close
quill-cli tab close notes.md
quill-cli tab close --discard
```

### tab next

```
quill-cli tab next
```

Show the next tab, wrapping round at the end.

```sh
quill-cli tab next
```

### tab previous

```
quill-cli tab previous
```

Show the previous tab.

```sh
quill-cli tab previous
```

### tab move

```
quill-cli tab move <position> [--tab <tab>] [--pane <number>]
```

Move a tab along its strip, or into another pane, which is what dragging it does. The position counts the tabs of the pane it is going to, as they are on the screen now.

- `position` — Where it goes, counting from 0. Past the end means the end.

- `--tab <tab>` — Which tab to move: its number, name or path. The tab that is showing when it is left out.
- `--pane <number>` — Which pane to move it into, counting from 0. The pane it is already in when it is left out.

```sh
quill-cli tab move 0
quill-cli tab move 0 --tab notes.md --pane 1
```

### tab save

```
quill-cli tab save
```

Write the tab that is showing back to its file.

```sh
quill-cli tab save
```

### tab save-as

```
quill-cli tab save-as <path>
```

Write the tab that is showing to another file, and go on editing that one.

- `path` — Where to write it.

```sh
quill-cli tab save-as notes/copy.md
```

### tab reload

```
quill-cli tab reload [--discard]
```

Read the file from disk again. A tab with unsaved changes is refused unless you say to throw them away, because there is no undo for that.

- `--discard` — Reload even though the tab has unsaved changes, losing them.

```sh
quill-cli tab reload
quill-cli tab reload --discard
```

## pane — the editing area split into panes

The editing area can be split into panes side by side, each with its own tabs, which is IntelliJ's split view. `pane split` moves the tab that is showing into a new pane on the right — it moves rather than copies, because two tabs on one file would be two documents over one path. A pane holding only that tab keeps it and the new pane opens empty, ready for the next file: opening a file always lands in the pane that has the keyboard.

### pane list

```
quill-cli pane list
```

The panes the editing area is split into, with the tabs in each, which tab is showing in each, and which pane has the keyboard.

```sh
quill-cli pane list --json
```

### pane split

```
quill-cli pane split
```

Put a pane to the right of the one with the keyboard and move the tab that is showing into it. A pane holding only that tab keeps it and the new pane opens empty.

```sh
quill-cli pane split
```

### pane move

```
quill-cli pane move <direction>
```

Move the tab that is showing into the pane beside it.

- `direction` — left or right.

```sh
quill-cli pane move right
quill-cli pane move left
```

### pane focus

```
quill-cli pane focus <pane>
```

Put the keyboard in a pane, so that the next file opened lands in it.

- `pane` — Its number counting from 0, left to right.

```sh
quill-cli pane focus 1
```

### pane width

```
quill-cli pane width <pane> <fraction>
```

Set one pane's share of the editing area, which is what dragging the divider between two panes does. The other panes share what is left.

- `pane` — Its number counting from 0.
- `fraction` — Its share of the width, between 0.05 and 0.95.

```sh
quill-cli pane width 0 0.35
```

### pane unsplit

```
quill-cli pane unsplit
```

Fold the pane that has the keyboard into the one beside it, keeping its tabs.

```sh
quill-cli pane unsplit
```

### pane unsplit-all

```
quill-cli pane unsplit-all
```

Put every tab back into one pane.

```sh
quill-cli pane unsplit-all
```

## editor — the text in the tab that is showing

These are about the tab that is showing. Lines and columns count from 1, which is what the status bar shows.

### editor status

```
quill-cli editor status
```

What the tab that is showing holds: its path, how many lines, where the caret is, what is selected, whether it has unsaved changes and which view mode it is in.

```sh
quill-cli editor status --json
```

### editor text

```
quill-cli editor text [--from-line <number>] [--to-line <number>]
```

Read the text of the tab that is showing.

- `--from-line <number>` — The first line to read, counting from 1.
- `--to-line <number>` — The last line to read, counting from 1.

```sh
quill-cli editor text
quill-cli editor text --from-line 1 --to-line 20
```

### editor set-text

```
quill-cli editor set-text [text] [--from-file <path>]
```

Replace everything in the tab that is showing. One undo puts it back.

- `text` (optional) — The new text. Use --from-file instead for anything long. Everything after it on the line belongs to it.

- `--from-file <path>` — Read the new text from this file rather than from the command line.

```sh
quill-cli editor set-text # Notes
quill-cli editor set-text --from-file draft.md
```

### editor insert

```
quill-cli editor insert <text>
```

Type text at the caret, replacing the selection if there is one.

- `text` — What to type. \n is a new line and \t is a tab. Everything after it on the line belongs to it.

```sh
quill-cli editor insert Hello
quill-cli editor insert "one\ntwo"
```

### editor caret

```
quill-cli editor caret [--line <number>] [--column <number>]
```

Read where the caret is, or move it. Lines and columns count from 1, which is what the status bar shows.

- `--line <number>` — The line to move to.
- `--column <number>` — The column to move to. The start of the line when it is left out.

```sh
quill-cli editor caret
quill-cli editor caret --line 42 --column 5
```

### editor select

```
quill-cli editor select [--all] [--none] [--from-line <number>] [--from-column <number>] [--to-line <number>] [--to-column <number>]
```

Select some of the text, all of it, or none of it.

- `--all` — Select the whole document.
- `--none` — Drop the selection, leaving the caret where it was.
- `--from-line <number>` — The line the selection starts on.
- `--from-column <number>` — The column it starts at. 1 when it is left out.
- `--to-line <number>` — The line it ends on.
- `--to-column <number>` — The column it ends at. The end of the line when it is left out.

```sh
quill-cli editor select --all
quill-cli editor select --from-line 3 --to-line 6
```

### editor undo

```
quill-cli editor undo
```

Undo the last edit in the tab that is showing.

```sh
quill-cli editor undo
```

### editor redo

```
quill-cli editor redo
```

Redo the edit that was last undone.

```sh
quill-cli editor redo
```

### editor view

```
quill-cli editor view <mode>
```

Choose how a file with a preview is shown: the source, the source and the preview side by side, or the preview. Markdown and Mermaid files have one; nothing else does, and only a file with a preview can be shown any way but raw.

- `mode` — raw, side or preview.

```sh
quill-cli editor view preview
quill-cli editor view side
```

### editor scroll

```
quill-cli editor scroll [--line <number>] [--to <points>] [--top] [--bottom] [--preview]
```

Read how far the tab that is showing is scrolled, or scroll it. With no flags it reports both halves of the side by side view. In side by side the other half follows, exactly as it does when you scroll with the wheel.

- `--line <number>` — Scroll so this line is at the top, counting from 1.
- `--to <points>` — Scroll to this many points down the page.
- `--top` — Scroll to the top.
- `--bottom` — Scroll to the bottom.
- `--preview` — Scroll the Markdown preview rather than the source.

```sh
quill-cli editor scroll --json
quill-cli editor scroll --line 120
quill-cli editor scroll --preview --top
```

### editor preview

```
quill-cli editor preview
```

Read the preview of the tab that is showing: a Markdown page as plain text with where its pictures and diagrams are, or, for a Mermaid file, what the diagram came out as.

```sh
quill-cli editor preview --json
```

### editor definition

```
quill-cli editor definition [--offset <bytes>] [--line <number>] [--column <number>] [--open]
```

Where the word at the caret is defined. Prints every candidate the project holds, best first, and --open goes to the best one. A file whose language has not said what a definition looks like has none, which is stated rather than guessed at.

- `--offset <bytes>` — Ask about this position in the file rather than about the caret.
- `--line <number>` — Ask about this line, counting from 1.
- `--column <number>` — The column on that line. 1 when it is left out.
- `--open` — Go to the best candidate, opening its file as a tab.

```sh
quill-cli editor definition --json
quill-cli editor definition --line 42 --column 9 --open
```

### editor references

```
quill-cli editor references [name] [--timeout <milliseconds>] [--code-only]
```

Every place a name is used across the project: the file, the line, the column and whether it is code or a word inside a comment or a string. Reads the open tabs as they stand and everything else from the disk.

- `name` (optional) — The name to look for. The word at the caret when it is left out.

- `--timeout <milliseconds>` — How long to wait for the search. 10000 by default.
- `--code-only` — Leave out the matches inside comments and strings.

```sh
quill-cli editor references --json
quill-cli editor references open_the_match --json
```

### editor rename

```
quill-cli editor rename <new-name> [--name <text>] [--scope <file|project>] [--include <comments,strings>] [--timeout <milliseconds>] [--apply]
```

Rename the word at the caret everywhere it is used. Prints the change set without --apply, so a script can look before it leaps; --apply edits the open tabs as documents, one undo step each, and rewrites the closed files on the disk.

- `new-name` — What to call it. It has to be a word of this language and not one of its keywords.

- `--name <text>` — Rename this name rather than the word at the caret.
- `--scope <file|project>` — Which files to change. The default follows what the name resolves to: a variable or a name with no known definition is this file, and a function, type, constant or module is the project.
- `--include <comments,strings>` — Also change the matches inside comments or strings, which are left alone by default.
- `--timeout <milliseconds>` — How long to wait for the search that finds them. 10000 by default.
- `--apply` — Make the change. Without it the change set is printed and nothing is edited.

```sh
quill-cli editor rename open_the_result --json
quill-cli editor rename open_the_result --apply
quill-cli editor rename total --scope project --include comments --apply
```

### editor complete

```
quill-cli editor complete [--offset <bytes>] [--line <number>] [--column <number>] [--limit <number>] [--choose <name>]
```

The names the word being typed at the caret could become, best first: the same list the popup shows, with what each row is and where it came from. Inside an import it is what can be imported instead — the project's files in a module specifier, and what a module exports between the braces — and there it answers with nothing typed. --choose applies one of them exactly as Enter would.

- `--offset <bytes>` — Ask about this position in the file rather than about the caret.
- `--line <number>` — Ask about this line, counting from 1.
- `--column <number>` — The column on that line. 1 when it is left out.
- `--limit <number>` — Print at most this many rows. All of them when it is left out.
- `--choose <name>` — Apply this row to the word being typed, as Enter would. It has to be one of the names offered.

```sh
quill-cli editor complete --json
quill-cli editor complete --limit 5 --json
quill-cli editor complete --choose draw_frame
quill-cli editor complete --choose ./layout
```

### editor navigate-back

```
quill-cli editor navigate-back
```

Go back to where the caret was before the last jump, reopening the file if its tab was closed.

```sh
quill-cli editor navigate-back
```

### editor navigate-forward

```
quill-cli editor navigate-forward
```

Undo a navigate-back. Cleared by any new jump, exactly as a browser's forward button is.

```sh
quill-cli editor navigate-forward
```

## highlight — the passages marked in the project's files

A highlight is a colour behind a passage of text. It stays there until it is cleared, in this file and next time the project is opened, and it moves with the text as the file is edited. These work on a file whether it is open or not, so `highlight apply` can mark twenty passages across twenty files in one call.

### highlight list

```
quill-cli highlight list [path] [--all]
```

What is marked, in one file or across the whole project: where each passage is, what colour it is in, and the text under it.

- `path` (optional) — The file to list. The tab that is showing when it is left out.

- `--all` — List every file in the project rather than one.

```sh
quill-cli highlight list --json
quill-cli highlight list --all --json
```

### highlight add

```
quill-cli highlight add [path] [--from-line <number>] [--from-column <number>] [--to-line <number>] [--to-column <number>] [--text <words>] [--color <name>]
```

Mark a passage in a colour. Give it lines and columns, or --text to mark every occurrence of some words. The file need not be open.

- `path` (optional) — The file to mark. The tab that is showing when it is left out.

- `--from-line <number>` — The line the passage starts on, counting from 1.
- `--from-column <number>` — The column it starts at. 1 when it is left out.
- `--to-line <number>` — The line it ends on. The line it started on when it is left out.
- `--to-column <number>` — The column it ends at. The end of the line when it is left out.
- `--text <words>` — Mark every occurrence of these words in the file instead of a range.
- `--color <name>` — yellow, green, blue, pink, or a colour of your own as #rrggbb or #rrggbbaa. Yellow when it is left out.

```sh
quill-cli highlight add --from-line 12 --to-line 18
quill-cli highlight add src/main.rs --from-line 40 --to-line 44 --color blue
quill-cli highlight add src/main.rs --text "unwrap()" --color pink
```

### highlight clear

```
quill-cli highlight clear [path] [--from-line <number>] [--to-line <number>] [--all]
```

Take marks away: a range of lines, a whole file, or every file in the project.

- `path` (optional) — The file to clear. The tab that is showing when it is left out.

- `--from-line <number>` — The first line to clear, counting from 1. The whole file when it is left out.
- `--to-line <number>` — The last line to clear. The line it started on when it is left out.
- `--all` — Clear every file in the project.

```sh
quill-cli highlight clear
quill-cli highlight clear src/main.rs --from-line 40 --to-line 44
quill-cli highlight clear --all
```

### highlight apply

```
quill-cli highlight apply [--from-file <path>] [--json-text <json>] [--replace]
```

Mark many passages across many files in one go, from a JSON array of {path, fromLine, toLine, fromColumn, toColumn, color} objects.

- `--from-file <path>` — Read the JSON array from this file.
- `--json-text <json>` — The JSON array itself, for a short list. Quote it.
- `--replace` — Clear every mark in the project first, so what is applied is all there is.

```sh
quill-cli highlight apply --from-file marks.json
quill-cli highlight apply --json-text '[{"path":"src/main.rs","fromLine":1,"toLine":3}]'
```

## terminal — the shells along the bottom

`terminal send` types into the shell and presses Enter; `terminal read --wait-for` is how to wait for what it did.

### terminal show

```
quill-cli terminal show
```

Show the terminal along the bottom, opening a shell in the project folder if there is not one already.

```sh
quill-cli terminal show
```

### terminal hide

```
quill-cli terminal hide
```

Put the terminal away. The shells keep running.

```sh
quill-cli terminal hide
```

### terminal toggle

```
quill-cli terminal toggle
```

Show the terminal if it is hidden, and hide it if it is showing.

```sh
quill-cli terminal toggle
```

### terminal new

```
quill-cli terminal new
```

Start another shell in a tab of its own, and show it.

```sh
quill-cli terminal new
```

### terminal list

```
quill-cli terminal list
```

The terminal tabs, with the name of each and which one is showing.

```sh
quill-cli terminal list --json
```

### terminal select

```
quill-cli terminal select <index>
```

Show one of the terminal tabs.

- `index` — Its number, counting from 0.

```sh
quill-cli terminal select 1
```

### terminal close

```
quill-cli terminal close [index]
```

Close a terminal tab. Closing the last one puts the terminal away.

- `index` (optional) — Its number. The tab that is showing when it is left out.

```sh
quill-cli terminal close
```

### terminal rename

```
quill-cli terminal rename <name> [--tab <index>]
```

Call a terminal tab something else. The name stays put when the program in the tab sets a title of its own; an empty name puts the tab back to being named after its program.

- `name` — What to call it. Everything after the verb is taken as the name, so it needs no quotes. Everything after it on the line belongs to it.

- `--tab <index>` — Which tab, counting from 0. The one that is showing when it is left out.

```sh
quill-cli terminal rename build
quill-cli terminal rename --tab 1 the long running one
```

### terminal move

```
quill-cli terminal move <position> [--tab <index>]
```

Move a terminal tab along the strip, which is what dragging one does.

- `position` — Where it goes, counting the tabs as they are on the screen now from 0.

- `--tab <index>` — Which tab to move, counting from 0. The one that is showing when it is left out.

```sh
quill-cli terminal move 0
quill-cli terminal move --tab 2 0
```

### terminal send

```
quill-cli terminal send [text] [--no-enter] [--key <name>]
```

Send a command to the shell in the terminal tab that is showing. Enter is pressed for you unless you say not to.

- `text` (optional) — The command. Everything after the verb is taken as the command, so it needs no quotes. Everything after it on the line belongs to it.

- `--no-enter` — Type the text and leave it on the prompt without running it.
- `--key <name>` — Send a key instead of text: enter, tab, escape, up, down, left, right, backspace, ctrl-c, ctrl-d, ctrl-l.

```sh
quill-cli terminal send git status
quill-cli terminal send --key ctrl-c
quill-cli terminal send --no-enter cd ..
```

### terminal read

```
quill-cli terminal read [--lines <number>] [--wait-for <text>] [--timeout <milliseconds>]
```

Read what the terminal tab that is showing has on its screen.

- `--lines <number>` — Only the last so many lines.
- `--wait-for <text>` — Wait until this text is on the screen before answering, which is how to wait for a command to finish.
- `--timeout <milliseconds>` — How long to wait for --wait-for. 10000 by default.

```sh
quill-cli terminal read --lines 20
quill-cli terminal read --wait-for "$" --timeout 15000
```

### terminal height

```
quill-cli terminal height [points]
```

Read how tall the terminal tile is, or set it. The same measurement dragging its top edge changes.

- `points` (optional) — How tall to make it. Read it when this is left out.

```sh
quill-cli terminal height 400
```

## run — the named commands the project is started with

A run configuration is a named command line, a folder and some environment variables, kept in the project. Starting one runs the program in a pseudoterminal, so `run output` is what it would have printed to a terminal — which is how to start a dev server, read the port out of its log, use it, and stop it, with nobody watching.

### run list

```
quill-cli run list
```

The project's run configurations: the name, the command, the folder and the environment of each, whether it is permanent, temporary or a suggestion, and what its run is doing.

```sh
quill-cli run list --json
```

### run add

```
quill-cli run add <name> <command> [--directory <path>] [--env <pairs>]
```

Keep a new run configuration in the project. The command is one line: the first word is the program and the rest are its arguments, and no shell runs it, so nothing is expanded and && is an argument.

- `name` — What to call it, which is what the widget and the Run menu show.
- `command` — The command line. Everything after the name is taken as the command, so it needs no quotes. Everything after it on the line belongs to it.

- `--directory <path>` — The folder it runs in, relative to the project. The project itself when it is left out.
- `--env <pairs>` — NAME=value pairs separated by semicolons.

```sh
quill-cli run add "Dev server" node server.js --port 3000
quill-cli run add build cargo build --release --directory crates/quill-app
quill-cli run add serve npm run dev --env "PORT=3000; DEBUG=app:*"
```

### run remove

```
quill-cli run remove <name>
```

Take a run configuration away. One whose program is still running is stopped first.

- `name` — The configuration, as `run list` gives it.

```sh
quill-cli run remove "Dev server"
```

### run start

```
quill-cli run start [name]
```

Run a configuration, showing the run tile. Starting one that is already running stops it and starts it again rather than making a second copy. A detector's suggestion started this way is kept as a temporary configuration.

- `name` (optional) — The configuration. The chosen one when it is left out.

```sh
quill-cli run start
quill-cli run start "Dev server"
```

### run stop

```
quill-cli run stop [name]
```

Stop a run: the interrupt a program can catch, and a hard kill two seconds later or on a second stop. The tab stays, holding what the program wrote.

- `name` (optional) — The configuration. The chosen one when it is left out.

```sh
quill-cli run stop
quill-cli run stop "Dev server"
```

### run rerun

```
quill-cli run rerun [name]
```

Stop a run and start it again, whatever state it was in.

- `name` (optional) — The configuration. The chosen one when it is left out.

```sh
quill-cli run rerun
```

### run select

```
quill-cli run select <name>
```

Choose which configuration the widget's play button, the Run menu and `run start` with no name all mean.

- `name` — The configuration.

```sh
quill-cli run select "Dev server"
```

### run output

```
quill-cli run output [name] [--tail <number>] [--wait-for <text>] [--timeout <milliseconds>]
```

What a run has written, as text. It ran in a pseudoterminal, so this is what it would have printed to a terminal — colours and progress bars included, with the escape sequences already read.

- `name` (optional) — The configuration. The run that is showing when it is left out.

- `--tail <number>` — Only the last so many lines.
- `--wait-for <text>` — Wait until this text has been written before answering, which is how to wait for a server to say it is listening.
- `--timeout <milliseconds>` — How long to wait for --wait-for. 10000 by default.

```sh
quill-cli run output --tail 20
quill-cli run output "Dev server" --wait-for "Listening on" --timeout 30000
```

### run status

```
quill-cli run status [name]
```

Whether a run is going, and what it ended with: running, finished, stopped, or the exit code it chose.

- `name` (optional) — The configuration. The chosen one when it is left out.

```sh
quill-cli run status --json
quill-cli run status "Dev server" --json
```

## explorer — the file tree down the left

`explorer files` is the list Quill searches, which leaves out `target`, `node_modules` and `__pycache__`.

### explorer show

```
quill-cli explorer show
```

Show the file explorer down the left.

```sh
quill-cli explorer show
```

### explorer hide

```
quill-cli explorer hide
```

Collapse the file explorer, leaving the rail of buttons.

```sh
quill-cli explorer hide
```

### explorer toggle

```
quill-cli explorer toggle
```

Show the explorer if it is hidden, and hide it if it is showing.

```sh
quill-cli explorer toggle
```

### explorer width

```
quill-cli explorer width [points]
```

Read how wide the explorer is, or set it. The same measurement dragging its edge changes.

- `points` (optional) — How wide to make it, from 150 to 620. Read it when this is left out.

```sh
quill-cli explorer width 320
```

### explorer filter

```
quill-cli explorer filter [text]
```

Read the explorer's filter box, or type into it. The tree then shows only what matches.

- `text` (optional) — What to filter by. The box is cleared when this is left out. Everything after it on the line belongs to it.

```sh
quill-cli explorer filter tdd
quill-cli explorer filter
```

### explorer expand

```
quill-cli explorer expand <path>
```

Open a folder in the tree, and every folder above it.

- `path` — The folder, relative to the project or absolute.

```sh
quill-cli explorer expand crates/quill-app/src
```

### explorer collapse

```
quill-cli explorer collapse [path]
```

Shut a folder in the tree.

- `path` (optional) — The folder. Every open folder is shut when this is left out.

```sh
quill-cli explorer collapse crates
quill-cli explorer collapse
```

### explorer tree

```
quill-cli explorer tree [--limit <number>]
```

The rows the explorer is showing, in order, with the depth of each and whether it is a folder.

- `--limit <number>` — At most this many rows. 200 by default.

```sh
quill-cli explorer tree --json
```

### explorer files

```
quill-cli explorer files [--limit <number>]
```

Every file in the project that Quill searches, which leaves out what a build wrote: target, node_modules and __pycache__.

- `--limit <number>` — At most this many paths. 500 by default.

```sh
quill-cli explorer files --limit 20 --json
```

### explorer select-open-file

```
quill-cli explorer select-open-file
```

Scroll the explorer to the file that is showing and select it, opening out the folders above it. It happens on its own when the tab changes; this asks for it by hand.

```sh
quill-cli explorer select-open-file
```

### explorer select

```
quill-cli explorer select [path]
```

Set the row the explorer's own cursor is on, which is what Delete is about, or read it when no path is given. It is not the same as the tab that is showing.

- `path` (optional) — The file or folder to select.

```sh
quill-cli explorer select README.md
quill-cli explorer select --json
```

### explorer delete

```
quill-cli explorer delete <path>
```

Delete a file or a folder. On Windows it goes to the Recycle Bin; everywhere else it is gone. No question is asked, because typing the command is the deliberate act the question exists to ask for.

- `path` — The file or folder to delete.

```sh
quill-cli explorer delete notes/old.md
```

### explorer move

```
quill-cli explorer move <path> <folder> [--dry-run] [--no-refactor]
```

Move a file or a folder into another folder, rewriting every import, use line and mod declaration in the project that names it. The same thing dragging a row in the explorer does.

- `path` — The file or folder to move.
- `folder` — The folder it goes into.

- `--dry-run` — Print the whole change set and change nothing at all.
- `--no-refactor` — Move the bytes and leave every reference to them alone.

```sh
quill-cli explorer move src/app/layout.ts src/draw
quill-cli explorer move src/app/layout.ts src/draw --dry-run --json
```

### explorer reveal

```
quill-cli explorer reveal <path>
```

Show a path in the platform's own file manager: Explorer on Windows, Finder on macOS.

- `path` — The file or folder.

```sh
quill-cli explorer reveal README.md
```

## modal — every dialog, driven the same way

One set of commands drives all of them: open it, type in it, read its results, choose a row, accept or cancel. A modal added to Quill later is driven with these same commands.

### modal list

```
quill-cli modal list
```

The modals that can be opened, and which one is open now.

```sh
quill-cli modal list --json
```

### modal open

```
quill-cli modal open <name> [--query <text>] [--path <path>] [--page <name>]
```

Open a modal, and put something in its box in the same breath.

- `name` — go-to-file, find-in-files, settings, about, new-file or rename.

- `--query <text>` — Type this into the modal's box as it opens.
- `--path <path>` — The folder a new file goes in, or the file being renamed. Needed by new-file and rename.
- `--page <name>` — Which page the Settings modal shows: appearance, editor, plugins, terminal or mcp.

```sh
quill-cli modal open go-to-file --query mdrs
quill-cli modal open find-in-files --query "fn main"
quill-cli modal open settings --page terminal
quill-cli modal open about
quill-cli modal open new-file --path notes
```

### modal state

```
quill-cli modal state
```

What the modal that is open is showing: its name, what is in its box, how many results it has and which one is chosen.

```sh
quill-cli modal state --json
```

### modal type

```
quill-cli modal type [text] [--match-case]
```

Put text in the box of the modal that is open, as though it had been typed.

- `text` (optional) — What to put in the box. The box is cleared when this is left out. Everything after it on the line belongs to it.

- `--match-case` — Turn on Find in Files' match case tick box while typing.

```sh
quill-cli modal type quill-cli
quill-cli modal type --match-case Quill
```

### modal results

```
quill-cli modal results [--limit <number>] [--wait <milliseconds>]
```

What the modal that is open has found: the files Go to File matched, or the lines Find in Files matched.

- `--limit <number>` — At most this many. 50 by default.
- `--wait <milliseconds>` — Wait up to this long for a search that is still running to finish.

```sh
quill-cli modal results --limit 10 --json
quill-cli modal results --wait 5000 --json
```

### modal choose

```
quill-cli modal choose <index>
```

Move the chosen row in the modal that is open, without opening anything.

- `index` — The row, counting from 0.

```sh
quill-cli modal choose 2
```

### modal accept

```
quill-cli modal accept [index]
```

Do what pressing Enter in the modal does: open the chosen file, jump to the chosen match, or press the modal's main button.

- `index` (optional) — Choose this row first.

```sh
quill-cli modal accept
quill-cli modal accept 0
```

### modal cancel

```
quill-cli modal cancel
```

Shut the modal that is open without doing anything, the way Escape does.

```sh
quill-cli modal cancel
```

### modal move

```
quill-cli modal move [--x <points>] [--y <points>]
```

Drag the modal that is open to a place on the window, the way its header does.

- `--x <points>` — How far from the left of the window its left edge goes.
- `--y <points>` — How far from the top of the window its top edge goes.

```sh
quill-cli modal move --x 60 --y 60
```

### modal size

```
quill-cli modal size [--width <points>] [--height <points>]
```

Resize the modal that is open, the way its edges do.

- `--width <points>` — How wide to make it.
- `--height <points>` — How tall to make it.

```sh
quill-cli modal size --width 900 --height 600
```

### modal reset

```
quill-cli modal reset
```

Put the modal that is open back in the middle at the size it asked for, the way a double click on its header does.

```sh
quill-cli modal reset
```

## settings — Edit -> Settings, by the names in the settings file

The names are the ones in Quill's own `settings.conf`, so there is one vocabulary rather than two. A change takes effect at once, in every tab, and is written to the file.

### settings list

```
quill-cli settings list
```

Every setting, with its value, what it means and what it will accept. The names are the ones in Quill's own settings file.

```sh
quill-cli settings list --json
```

### settings get

```
quill-cli settings get <key>
```

Read one setting.

- `key` — The name, such as appearance.font.size.

```sh
quill-cli settings get appearance.font.size
```

### settings set

```
quill-cli settings set <key> <value>
```

Change one setting. It takes effect at once, in every tab, and is written to the settings file.

- `key` — The name, such as appearance.background.opacity.
- `value` — The new value. Everything after it on the line belongs to it.

```sh
quill-cli settings set appearance.font.size 20
quill-cli settings set appearance.background.opacity 0.5
quill-cli settings set editor.line_numbers false
quill-cli settings set terminal.shell cmd.exe
quill-cli settings set appearance.font.family "Courier New"
```

### settings reset

```
quill-cli settings reset [key]
```

Put a setting, or every setting, back to what a Quill that has never been run has.

- `key` (optional) — The setting. All of them when it is left out.

```sh
quill-cli settings reset appearance.font.size
quill-cli settings reset
```

### settings fonts

```
quill-cli settings fonts [--limit <number>]
```

The font families this machine has that the editor can be set to.

- `--limit <number>` — At most this many. 100 by default.

```sh
quill-cli settings fonts --json
```

## plugins — the languages Quill colours

A plugin describes a language: its extensions, its keywords and a colour per kind of token. Nothing in one is executed and nothing is fetched over a network.

### plugins list

```
quill-cli plugins list
```

The language plugins Quill has, which of them are switched on, and what each one claims. They ship with Quill; nothing is fetched.

```sh
quill-cli plugins list --json
```

### plugins install

```
quill-cli plugins install <id>
```

Write a plugin out into the settings folder, so its files can be read and changed.

- `id` — The plugin's id, as `plugins list` gives it.

```sh
quill-cli plugins install rust
```

### plugins enable

```
quill-cli plugins enable <id>
```

Switch a plugin on, so it colours the files it claims.

- `id` — The plugin's id.

```sh
quill-cli plugins enable rust
```

### plugins disable

```
quill-cli plugins disable <id>
```

Switch a plugin off. Its files stay where they are.

- `id` — The plugin's id.

```sh
quill-cli plugins disable rust
```

## git — the Git menu

Git runs on a thread, so an action is asked for and `git status` says what came back. `--wait` holds the answer open until it has.

### git status

```
quill-cli git status
```

What git says about the project: the branch, whether a merge or a rebase is unfinished, and what the last command it was asked for came back with.

```sh
quill-cli git status --json
```

### git actions

```
quill-cli git actions
```

Everything on the Git menu, by the name `git action` takes.

```sh
quill-cli git actions --json
```

### git action

```
quill-cli git action <name> [--path <path>] [--wait <milliseconds>]
```

Run one of the entries on the Git menu. Git runs on a thread, so the answer says it was asked for, and --wait holds on for what came back.

- `name` — The entry, such as commit, push, pull, fetch, branches or annotate.

- `--path <path>` — The file it is about. The file that is showing when it is left out.
- `--wait <milliseconds>` — Wait up to this long for git to answer before returning.

```sh
quill-cli git action fetch --wait 20000
quill-cli git action annotate
quill-cli git action show-history --path README.md
```

## action — every menu entry there is

The escape hatch, and the guarantee: every entry on every menu has a name here, and the list is built by walking the real menus, so a menu entry added to Quill tomorrow can be run from the command line tomorrow.

### action list

```
quill-cli action list
```

Every entry on every menu, with the name `action run` takes, the menu it is on, its keyboard shortcut and whether it can be used just now. A new menu entry appears here without anybody adding it.

```sh
quill-cli action list --json
```

### action run

```
quill-cli action run <name> [--path <path>]
```

Run a menu entry by name. This is the way to reach something with no command of its own; the entries that would open a file chooser are refused, and the answer says which command to use instead.

- `name` — The entry, as `action list` gives it, such as toggle-line-numbers.

- `--path <path>` — The file or folder the entry is about, for the ones that take one.

```sh
quill-cli action run toggle-line-numbers
quill-cli action run about
```

## project — the folder this window is showing

A project is a window. Opening a second project is `quill-cli launch <folder>`, which starts a second Quill; `project open` changes the folder this window is showing.

### project open

```
quill-cli project open <folder>
```

Show another folder in this window. What was open in the project being left is written down first.

- `folder` — The folder to show.

```sh
quill-cli project open C:\jason\dev\quill
```

### project recent

```
quill-cli project recent
```

The projects that have been open, newest first.

```sh
quill-cli project recent --json
```

## mcp — the server an AI agent drives Quill through

The Model Context Protocol server, which is how an AI agent discovers and drives Quill without being handed a document first. Its tools are generated from this same catalogue, so a command added to Quill is a tool the day it is added.

### mcp serve

```
quill-cli mcp serve [--transport <stdio|http>] [--port <number>] [--tools <grouped|every>] [--instance <which>]
```

Run the Model Context Protocol server, which is how an AI agent drives Quill. Over stdin and stdout by default, which is what an agent that launches it wants; over HTTP with `--transport http`.

- `--transport <stdio|http>` — How the client talks to it. `stdio` by default.
- `--port <number>` — Which port to listen on, for `--transport http`. 7345 by default.
- `--tools <grouped|every>` — One tool per area, or one tool per command. `grouped` by default.
- `--instance <which>` — Which running Quill to drive, when several are running.

```sh
quill-cli mcp serve
quill-cli mcp serve --transport http --port 7345
```

Answered by the CLI itself; no Quill needs to be running.

### mcp status

```
quill-cli mcp status
```

What this Quill is doing about MCP: whether it is serving over HTTP, on which port, in which tool shape, how many tools that is, and where an agent's configuration should point.

```sh
quill-cli mcp status --json
```

### mcp install

```
quill-cli mcp install <client> [--transport <stdio|http>] [--port <number>] [--scope <user|project>] [--name <name>] [--remove]
```

Write Quill's MCP server into an agent's own configuration, so it is there next time the agent starts.

- `client` — `claude`, `codex`, or `both`.

- `--transport <stdio|http>` — Which way the agent should talk to it. `stdio` by default, which needs no port.
- `--port <number>` — The port to point at, for `--transport http`.
- `--scope <user|project>` — `user` for every project, `project` for this folder only. `user` by default.
- `--name <name>` — What the server is called in the agent's configuration. `quill` by default.
- `--remove` — Take it out again rather than putting it in.

```sh
quill-cli mcp install both
quill-cli mcp install claude --scope project
```

Answered by the CLI itself; no Quill needs to be running.

### mcp config

```
quill-cli mcp config [client] [--transport <stdio|http>] [--port <number>] [--name <name>]
```

Print the configuration to paste into an agent that has no button of its own: the JSON an `mcpServers` block wants, and the TOML Codex wants.

- `client` (optional) — `claude` or `codex`. Both when it is left out.

- `--transport <stdio|http>` — Which way to describe. `stdio` by default.
- `--port <number>` — The port to name, for `--transport http`.
- `--name <name>` — What to call the server. `quill` by default.

```sh
quill-cli mcp config
quill-cli mcp config codex --transport http
```

Answered by the CLI itself; no Quill needs to be running.

### mcp tools

```
quill-cli mcp tools [--tools <grouped|every>] [--count]
```

The tools the MCP server offers, exactly as it would answer `tools/list`. This is how to see what an agent will be given, and how the cost of the two shapes is compared.

- `--tools <grouped|every>` — Which shape to print. `grouped` by default.
- `--count` — Print how many tools and how large the list is, rather than the list.

```sh
quill-cli mcp tools --json
quill-cli mcp tools --tools every --count
```

Answered by the CLI itself; no Quill needs to be running.


<!-- end generated reference -->
