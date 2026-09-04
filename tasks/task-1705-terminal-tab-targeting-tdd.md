# task-1705 — `terminal send` and `terminal read` take a tab, and the area settles on one way of naming one

The task-1695 study watched a local model drive a real window, and in the terminal scenario it did
the sensible thing and then hit a wall. It made a new terminal tab, named it `checks`, and wrote
`{"command":"send","text":"cargo check","tab":1}` — and was refused with
*"terminal send has no tab. terminal send takes text, no-enter, key."* The tab it had just made and
named was the one it could not speak to, and the only way to reach it was to make it the tab that is
showing first.

## 1. The fault, and why it is a race rather than a round trip

One area, four ways of naming a tab:

| verb | how a tab is named |
|---|---|
| `rename`, `move` | `--tab <index>` |
| `close`, `select` | a positional |
| **`send`, `read`** | **cannot — always the tab that is showing** |

The two verbs where targeting matters most are the two that lack it. `send` is how a command gets to
a shell and `read` is how the answer comes back, and both of them are pinned to whichever tab happens
to be showing. So an agent that wants to run a build in one tab and a dev server in another has to
`terminal select` first, and between the select and the send anything else driving the window — a
person, another command, a tab that closes itself when its shell exits — can change which tab is
showing, and the command lands in the wrong shell. That is a race, not just an extra round trip, and
the round trip is the cheap half of it.

`read` has the same fault in a second place: `--wait-for` holds the request open until the text is on
the screen, and while it waits the tab that is showing is the one it keeps looking at. A wait started
on one tab and answered by another is a wait that cannot be trusted.

## 2. What other tools do about this

The pattern is the same everywhere a multiplexer is built to be driven by a program:

- **tmux** — the model. Its man page says most commands take an optional `-t` naming the
  target-pane, and that *if the pane index is omitted, the currently active pane is used*. The same
  flag shape is shared by every pane-targeting verb: `send-keys -t`, `capture-pane -t` (the read),
  `select-pane -t`, `kill-pane -t`, `resize-pane -t`. One convention across the area, optional,
  defaulting to the active pane. `send-keys` and `capture-pane` are exactly `send` and `read`.
- **iTerm2's Python API** — a script takes the specific `Session` object it wants, out of
  `get_sessions()` or by id, and calls `send_text` on it. Targeting is explicit there because the
  object is the target; the principle is the same, that a send goes where it is named rather than
  where the pointer is.
- **tmuxr (R)** — states the default in one line: *target … if NULL, the currently active pane is
  used.*

The convention this settles on is tmux's, in Unluminous's spelling: **`--tab <index>`**, counting from 0,
optional, defaulting to the tab that is showing, and the one way every tab-targeting verb names a
tab. It is not a new mechanism so much as the mechanism `rename` and `move` already use, extended to
the four verbs that did not have it.

Two rules settle the awkward cases:

- **The flag is the name; the positional is a convenience.** `select` and `close` take a positional
  index today and keep it, because a script that works today must keep working. When both are given,
  the flag wins, because the flag is the settled convention and the positional is the thing being
  kept for the old callers. The window reads `--tab` first and falls back to the positional, which is
  one line rather than a second way to answer the same question.
- **Naming a tab does not show it.** `send --tab 1` sends to tab 1 and leaves the tab that is showing
  alone, which is the whole point of targeting: to speak to a shell without disturbing the one a
  person is looking at. `read --tab 1` reads tab 1 for the same reason. `select` is still the one
  verb that changes which tab is showing, and it is the only one that should.

## 3. The changes

### 3.1 `send` and `read` take `--tab`

Both gain `option("tab", "index", …)` in the catalogue, and both read the tab through the helper the
area already has, `cli_terminal_tab`, which answers `--tab` when it is given and the active index
otherwise, and `None` when there is no terminal at all — which is a different thing to be told than a
number that is out of range, and is why the helper does not answer with the active index blindly.

- `send` sends its bytes to that tab's session rather than to `active()`, and says which tab it sent
  to in the reply, so a caller that named a tab gets back the confirmation that it was that tab.
- `read` reads that tab's screen, and `--wait-for` carries the tab with the wait, so a hold that
  started on tab 1 is answered by tab 1 and by nothing else. The wait is `Waiting::TerminalText`,
  which gains a `tab` field for exactly this: a held request is a promise about a particular screen,
  and a promise that can be kept by looking at a different screen is not a promise.

The default of the flag is the old behaviour, so nothing that works today changes: a `send` with no
`--tab` goes to the tab that is showing, exactly as it did.

### 3.2 `select` and `close` accept `--tab` too

Both gain the same flag and read it through the same helper, with the positional as the fallback.
`select --tab 1` and `select 1` do the same thing; `close --tab 1` and `close 1` do the same thing.
`rename` and `move` already have the flag and are untouched. After the change, every verb that names
a tab names it the same way, and the two that also take a positional keep it for the callers that
have it.

### 3.3 The tab is reached by number, and the number is checked

`unluminous_terminal::Tabs` gains `at(index)` and `at_mut(index)`, which answer `Option<&Session>` and
`Option<&mut Session>` for a number, the way `active()` does for the active index. A number past the
end is `None`, and the verbs answer with the not-found refusal they already use for the same
condition, so a `send --tab 9` says *there is no terminal tab 9* rather than sending to the tab that
happens to be showing.

## 4. What this is not

- **Not a change to which tab is showing.** Targeting a tab for a send or a read does not select it,
  because selecting it is the one side effect a caller that named a tab did not ask for, and it is
  what made the old way a race. `select` remains the verb for that.
- **Not a name-based target.** tmux targets a pane by index, id or name, and a name is tempting
  because the study's agent had just *named* its tab `checks`. But a name is a second thing to keep
  in step with the strip — a rename, a move or a close all change what a name points at — and the
  index is what `terminal list` already hands back and what every other verb in the area takes. The
  name is for a person to read; the number is for a caller to use.
- **Not a change to the reply's shape for a caller that does not name a tab.** The default of the new
  flag is the old answer, and the one addition to a `send` reply — which tab it went to — is the
  confirmation a caller that named a tab needs and is harmless to one that did not.

## 5. How it is verified

1. The catalogue's own tests: every example parses, every command is documented with its current
   usage line, and the MCP tools are generated from the list — so `--tab` is on the `terminal` tool
   the day it is in the catalogue, with no tool written by hand.
2. Window tests over the real channel, in `tests/screenshots.rs` beside the ones that already drive
   the terminal verbs: a `send --tab` to a tab that is **not** showing lands in that tab's screen and
   leaves the showing tab alone; a `read --tab` reads the tab that is not showing without selecting
   it; a `read --tab --wait-for` holds for the named tab and is answered by it; `select --tab` and
   `close --tab` do what their positional spellings do; and a `--tab` past the end is refused.
3. The unit tests in `unluminous-terminal` for `at` and `at_mut`, beside the ones that already cover
   `active`.
4. A real window with two terminal tabs running different things, driven by `unluminous-cli`: a command
   sent to each by number, read back from each by number, with the tab that is showing never
   changed, is the acceptance test the ticket asks for.
