# task-1767 — the Agent-Chat plugin

A pane docked to the right of the window, opened from a button in the rail, in which you talk to a
model: your words go up, the answer comes back a token at a time, a picture can be attached to
either, and what the model asks Quill to do is shown as it happens. Which model, at which URL, is a
row on a Settings page rather than a constant in the binary.

The ticket asks for four things and names a fifth by pointing at another one:

> an agent chat plugin that opens as a right panel (left side toggle icon), can be moved around like
> the other panels, and looks like our ai service LLM chat, supports streaming, images, has a config
> in settings to allow configure url for Claude, codex etc. It should be dark neomorphic like our
> agent tasks plugin.

This document is what was weighed, what was chosen, and what was deliberately left out.

---

## 1. What it is measured against

`_agent_output/task-1767-agent-chat/reference-chat.png` is the picture, and
`_agent_output/task-1767-agent-chat/reference-chat.html` is how it was made. Both need explaining,
because the picture is not a photograph.

The ai-service LLM chat page is served from `localhost:7070` behind a browser login. The shared
`x-skip-token` this machine's agents hold opens `@LocalToken()` routes and nothing else, so an
agent cannot photograph that page. What an agent *can* read is the page itself:
`ui/src/components/llm/chat-page/` is fourteen components and their module CSS, and
`ui/src/app/neumorphic-tokens.css` and `ui/src/app/incognito-theme.css` are the two files every
shadow, radius and colour on it comes from.

So the reference was rebuilt rather than captured: the same structure — `ChatPanel` wrapping
`ChatHeader`, `ChatConversation` and `ChatComposer`, with `ChatMessage`, `StatusTopicEl`,
`ChatComposerToolbar` and `PromptInput` inside them — with the module CSS's own values, wearing the
**dark** token set rather than the light one, laid out at the width the pane will really have. That
is the same method `task-1765` used, and it is more exact than a photograph would have been: a
photograph would have to be measured back into numbers, and these numbers were never left.

The ticket is explicit that the *look* comes from somewhere else — "It should be dark neomorphic
like our agent tasks plugin (our ai service LLM chat page is the white neumorphic). Task 1765 has a
screenshot of the dark neomorphic look we want, and has the ui library we want to use." So there are
two references and each answers a different question: **the LLM chat page says what the surface is
made of**, and `_agent_output/task-1765-vello-board/reference-board.png` says **what it is made
of it in**.

### 1.1 What the LLM chat page is, part by part

| Part | What it is there | Where it comes from |
|---|---|---|
| The panel | one raised card, `--surface-1`, radius `--r-lg`, `--e-raised-sm`, holding all three rows | `ChatPanel.module.css` |
| The header | a flat row inside it: a mint status dot with a halo, the conversation's name, uppercase mono chips naming the datasources, a ghost `+` for a new chat, and a hairline rule under the lot | `ChatHeader.module.css` |
| The conversation | a scrolling column, 14 points between rows | `ChatConversation.module.css` |
| A message from you | right aligned, at most 75% of the width, `--e-raised-sm`, its top-**right** corner squared off to 6 | `ChatMessage.module.css` `.messageRowUser` |
| A message from the model | left aligned, at most 85%, `--e-pressed-sm`, its top-**left** corner squared off | `.messageRowAI` |
| A code block in one | `--e-pressed`, deeper than the bubble it is in, mono, wrapped | `.chatMessageLLM pre` |
| Inline code | a chip on `--surface-sunken` in `--accent-blue` | `.chatMessageLLM code` |
| A picture in one | `--e-raised-sm`, radius `--r-md`, capped at 400 points tall | `.chatMessageImage` |
| A tool call | a `--e-pressed-sm` block with a round raised icon, a title, a caret, and its updates indented under it, collapsed once it is finished | `StatusTopicEl.module.css` |
| The composer's toolbar | a pill of round icon buttons, `--e-pressed-sm`; a button that is on gains `--e-raised-sm` and its own accent colour | `ChatComposerToolbar.module.css`, `ChatTool.module.css` |
| The context meter | a thin `--e-pressed-sm` track with a `--grad-progress` bar in it | `ContextUsageMeter` |
| The prompt | a deep well, `--e-pressed`, with the send button as a gradient disc carrying a blue glow | `PromptInput.module.css` |
| Nothing said yet | a `--e-pressed-sm` rounded square holding an icon, a heading, a line under it and four starter chips | `ChatPage.module.css` `.chatEmpty` |

Every one of those is a soft shadow, an inset shadow, a gradient or a glow. There is not a flat
rectangle with a hard edge anywhere in it — which is the sentence `task-1765` ends its own
measurements with, and it is why this plugin asks for the same renderer.

### 1.2 The palette is Quill's, and it gains nothing

The dark token set and Quill's own colours line up almost exactly, and the ladder
`plugin_ui::Palette` grew for the board is the ladder a chat needs:

| The chat's surface | The dark token | `Palette` | Quill's colour |
|---|---|---|---|
| The pane behind the panel | `--surface-0` `#1C1F25` | `board_page` | `EDITOR` `#1A1F26` |
| The panel | `--surface-1` `#23272E` | `board_lane` | `EXPLORER` `#1F232A` |
| A message bubble | `--surface-1` raised or pressed | `board_card` | `CODE_PANEL` `#232933` |
| A well: the prompt, the toolbar pill, a code block, a tool block | `--surface-sunken` `#191C21` | `board_well` | `FIELD` `#1D212A` |
| Inline code | — | — | `CODE_CHIP` `#282F3A` |
| The send button | `--grad-primary-160` | `board_accent` | `BOARD_ACCENT` `#4C6EF5` |
| A tool that is running | `--accent-mint` | `attached` | `GIT_ADDED` |
| A tool that is a write | `--accent-violet` | `agent` | `AGENT` |
| A refusal | `--danger` | — | `CLOSE` |

**No colour is added.** That is worth saying plainly, because `task-1765` added three and said why
each one had to be new; this adds none, and the reason is that `task-1765` already added them. A
second blue for a plugin's own buttons is a decision that was made once.

---

## 2. Where the code goes, and why some of it is a crate of its own

### 2.1 `crates/quill-chat` — the protocol, the conversation and the thread

A new crate, and the argument for it is the argument `quill-dap` already makes. Reading a stream of
server-sent events, turning it into a conversation, and knowing what state a request is in are all
things that can be tested **with no window, no graphics card and no fonts**, and they are the half
of this feature most likely to be wrong. `quill-dap`'s own tests run against scripted adapters with
no process; `quill-chat`'s run against a scripted server with no network beyond loopback, and a few
run against no server at all because the framing is a pure function.

| Module | What is in it |
|---|---|
| `provider.rs` | what an endpoint is: a name, a wire shape, a URL, a model, where the key comes from, a system prompt, a token budget |
| `wire.rs` | building a request and reading a reply, for both shapes, as `serde_json::Value` |
| `sse.rs` | the `data:` line framing, incremental, over a byte stream that arrives in arbitrary pieces |
| `model.rs` | `Conversation`, `Message`, `Role`, `Part`, `ToolCall`, `Usage` |
| `session.rs` | the state machine: what a `Reply` does to a conversation |
| `client.rs` | the thread the request runs on, the waker, and the one place `ureq` is named |
| `base64.rs` | encoding a picture into a request and decoding one out of a data URL |

What must never be in it: any user interface dependency, for the same reason as the four crates
above it in the table in `CLAUDE.md`.

### 2.2 `quill-app` — the provider and the drawing

`services/agent_chat/` is the `UiProvider`: the conversations it holds, the settings it reads and
writes, the pictures it has been given, and the one place a command becomes a change.
`components/agent_chat/` is the drawing, one file a part, exactly as `components/agent_tasks/` is.

`crates/quill-app/plugins/agent-chat/plugin.conf` is the manifest. It is **data**: it says there is
a pane, which side it docks to, what its button looks like, what its menu holds and that it wants
the `vello` renderer. It cannot name a colour, and nothing in it is executed. That is the property
the plugin system has had since it was written and this does not give it up.

---

## 3. Talking to a server, which Quill has never done before

This is the first thing in Quill that makes an outbound network request, and that deserves to be
said out loud rather than slipped in.

Every existing rule about the network is a rule about something happening **without being asked**: a
Markdown preview must not fetch a picture, a diagram must not fetch anything, a plugin must not be
downloaded, a debug adapter must not be downloaded. `task-1692` drew the line where it belongs — "a
package manager the person pressed a button for, in a terminal they can watch, is not the editor
reaching out". A chat pane is the same shape and more so: the request is the feature, it goes to an
address the person typed into a Settings page, and it happens when they press send. Nothing here
fetches anything at startup, on a keystroke, or on a file being opened.

### 3.1 The client is `ureq`, over the machine's own TLS

Three routes were weighed.

**The operating system's own stack** — WinHTTP on Windows, `NSURLSession` on macOS — is what
`services/windows_transparency.rs` and `services/native_menu.rs` already do for their platforms, and
it would add no crate at all. It would also be two implementations of chunked reads, two of TLS
error reporting and two of proxy handling, both written in `unsafe`, and neither testable from the
other platform. The feature would then be twice as likely to be wrong in the half nobody could see.

**`reqwest`** is the obvious crate and brings `tokio` with it. Quill has no async runtime anywhere;
`quill_git::Worker`, the terminal's reader loop, `services::text_search`, `services::symbol_index`
and `quill_dap`'s reader are all plain threads with channels. A runtime added for one pane would be
a second concurrency model in a program that has one, and it is a large one.

**`ureq` 3 with `native-tls`** is what is used. It is blocking, which is what a worker thread wants;
its response body is a `Read`, which is what an incremental parser wants; and `native-tls` is
**schannel on Windows and Security.framework on macOS**, so the certificates Quill trusts are the
certificates the machine trusts. That is `quill-git`'s own argument made again: git is shelled out
to rather than reimplemented because the machine's git already knows about this machine's
credential helper, its ssh agent and its proxy, and a push from Quill has to be the same push you
get in the terminal. A request from Quill should reach the same servers `curl` reaches, including
behind whatever inspects the traffic on a corporate network, and the way to get that is to use the
store the machine already keeps rather than to ship a copy of the internet's root certificates.

Measured on this machine, that is **thirty-one crates** in the tree, of which `schannel`,
`windows-sys`, `serde_json`, `percent-encoding`, `http` and `base64` were already there. The rustls
route was measured too and is 121 packages with its own root store; the numbers are in §11.

`gzip`, `json`, `cookies`, `multipart` and `socks-proxy` are all switched off. Nothing here posts a
form, nothing keeps a cookie, and the bodies are built and read as `serde_json::Value`s by hand —
which is the rule `services::control` already keeps for the control channel, and for the same
reason: this is a small open protocol somebody else defined, not a Rust type going over a wire.

### 3.2 Streaming is a state machine over a byte stream, not a line reader

Server-sent events look line-oriented and are not: a chunk from a socket can end in the middle of a
UTF-8 character, in the middle of a `data:` line, or exactly on the blank line that ends an event.
`quill_chat::sse::Reader` therefore holds a byte buffer, is fed whatever arrived, and yields whole
events. It is a pure function of its input and its tests feed it the same stream split at every
byte boundary and assert the same events come out — which is the test `quill_dap`'s
`Content-Length` framing already has.

Two shapes are read from the events, and they are genuinely different protocols rather than one with
two spellings:

- **OpenAI** (`/v1/chat/completions`): one event per delta, `choices[0].delta.content` a string,
  `choices[0].delta.tool_calls[]` accumulating a function name and a *string* of arguments across
  many deltas, `data: [DONE]` at the end. Usage arrives only if `stream_options.include_usage` was
  asked for, which Quill asks for.
- **Anthropic** (`/v1/messages`): named events — `message_start`, `content_block_start`,
  `content_block_delta` (either `text_delta` or `input_json_delta`), `content_block_stop`,
  `message_delta` carrying the stop reason and the output tokens, `message_stop`. Blocks are
  indexed, so text and a tool call can interleave and each is accumulated against its own index.

Both are read into the **same** `Reply` values, so `session.rs` has one state machine and
`components/agent_chat` has never heard of either protocol:

```
Reply::Started { model }
Reply::Text(String)          // a delta, appended
Reply::Thinking(String)      // a delta of reasoning, kept apart from the answer
Reply::ToolCall { id, name, arguments }   // complete, once its block has stopped
Reply::Usage { input, output }
Reply::Finished { reason }
Reply::Failed(String)
```

**A refusal is the server's own words.** A 401, a 404, a 429, a model that does not exist, a URL
with a typo in it — each explains itself better than Quill could, so the body of a failed response
is what the pane shows, cut to a sentence with the whole of it under a disclosure. That is
`quill-git`'s rule ("nothing invents an error message") applied to a second kind of program Quill
does not control.

### 3.3 The thread, and how the window finds out

`quill_chat::Client::send` spawns a thread, exactly as `quill_git::Worker` does. It owns the
request, reads the body incrementally, and pushes `Reply`s down an `mpsc` channel; after each push
it calls the waker the provider was handed in `plugin_ui::Context`, which is `egui`'s
`request_repaint`. `UiProvider::catch_up` drains the channel once a frame and answers `true` while
a request is in flight, which is what keeps the window drawing while an answer is arriving.

**Stopping is dropping.** A stop flag shared as an `AtomicBool` is checked between reads; the
thread ends, the body is dropped and the connection closes. There is no request to a server to
cancel: HTTP has no such thing, and every one of these APIs treats a closed connection as a
cancellation. What is kept is whatever text had already arrived, marked as stopped — because
throwing away half an answer somebody was reading is worse than keeping it.

**Only the newest request is answered.** Each carries a generation number, the newest is shared with
the thread as an `AtomicU64`, and replies from a passed generation are dropped. That is
`services::text_search`'s own arrangement, and it is what makes "send, change your mind, send again"
work with no timer anywhere.

### 3.4 Where the key is, which is nowhere Quill writes

`services::agent_tasks::keychain` says it plainly: a secret is never written into a settings file,
because a settings file is copied between machines, readable by anything that can read the folder,
and pasted into a bug report. It also says, honestly, that **there is no Windows keychain in Quill**
— on Windows `read` answers `None` and `write` refuses with a sentence.

That leaves this pane, on the platform Quill is developed on, with nowhere safe to put a key. So it
does not put one anywhere: **a provider names an environment variable, and Quill reads it at the
moment a request is sent.**

- `provider.key_env = ANTHROPIC_API_KEY` — the name every tool on this machine already uses, and the
  variable `claude` itself reads.
- On macOS and Linux a provider may name a keychain entry instead, through the code that is already
  there; on Windows that row is **absent** rather than present and refusing, which is Quill's rule
  for a control that cannot apply.
- A local endpoint names neither, because llama.cpp wants no key.

So the settings file holds the **name** of the place a key is, exactly as Agent-Tasks' does, and on
every platform there is at least one way to have one. The Settings page shows `set` or `not set` and
never the value, the value is never logged, and a failed request's message never quotes a header.

---

## 4. The pane

### 4.1 It is a pane, and the machinery for that already exists

`app::plugin_panes`, `app::dock`, `components::dock` and `components::activity_bar` were all built
by the UI-plugin architecture and are still there — Agent-Tasks stopped asking for a pane in
`task-28` and everything it used remained. So the whole of the ticket's first sentence is a
manifest:

```
pane.id      = chat
pane.label   = Agent-Chat
pane.icon    = chat
pane.side    = right
pane.group   = top
pane.width   = 420
pane.applies = always
```

`pane.side = right` is the right panel. `pane.group = top` puts its button in the upper group of the
rail, which is the left side toggle icon. Dragging its header to another edge, the four blue drop
bands, the `Move to` menu on its header and its rail button, `Reset Panel Layout`, and
`quill-cli plugins pane agent-chat/chat --side bottom` are all `task-1697`'s and need no code here.

`pane.icon = chat` is the one new name: `plugins::PANE_ICONS` has ten and none of them is a speech
bubble. `theme::icon::chat` draws one — a rounded box with two lines in it and a tail, at the size
`icon::board` is drawn at — and it is *drawn* rather than lettered, which is what
`design/style-guide.md` requires and what lets it take the rail's three tints.

### 4.2 The four rows, and one rule about the ground

The pane's own header is the window's (`components::agent_tasks::pane_header`, which every
contributed pane gets). Under it the provider draws:

```
  header       32  the state dot, the conversation's name, the provider chip, history, new
  conversation  *  a scrolling column of rows
  composer      -  the toolbar pill, the meter, the attachments, the prompt
```

**The ground is already painted and the provider must not paint its own.** `show_the_plugin_panes`
fills the pane in `palette.editor` with the window's opacity, then reserves the decoration's slot
with `Painter::add(Shape::Noop)`, and only then calls the provider. A second ground painted here
would go into the painter *after* that slot and wash the decoration out — which is exactly what
happened to the board, and it was invisible for a while only because the ground carries the window's
opacity and let some of the decoration through. So `panel` is recorded through `Chrome::raised` and
nothing else fills the pane.

### 4.3 A row in the conversation

Five kinds, and each is a value in `model.rs` rather than a shape the drawing decides:

| Row | Drawn |
|---|---|
| `Role::User` | right aligned, at most 78% of the width, `Chrome::raised(..., Lift::Small)` on `board_card`, top-right corner squared to 6 |
| `Role::Assistant` | left aligned, at most 92%, `Chrome::sunken(..., Lift::Small)`, top-left squared |
| a tool call | full width less a small indent, `Chrome::sunken` on `board_well`, a raised round icon disc, a caret, and its arguments and result as mono rows |
| thinking | the quiet colour, italic, behind a disclosure, collapsed once the answer starts |
| a failure | `Chrome::sunken` with a `CLOSE` coloured left edge and the server's own words |

The body of a message is **markdown**, through `components::markdown_text`, which is the same
`quill_core::markdown::render` the editor's own preview is made of. So headings, lists, quotes,
tables and fenced code all work, a fence is coloured by whichever plugin claims its language through
the `CodeHighlighter` seam `PluginHighlighter` already implements, and none of it is a second
renderer. What it does **not** do is pictures and Mermaid diagrams inside a message body, for the
reason `components/markdown_text.rs` already records: resolving those needs two further passes that
decode an image and lay a diagram out. A picture **attached** to a message is drawn — see §6 — and
one written as `![](…)` inside the text shows its alt text. `plugin.limitations` says so.

### 4.4 Scrolling follows the answer, until you stop it

The column is scrolled to the bottom while an answer is arriving, and stops following the moment
somebody scrolls up — which is `ChatPage.tsx`'s own `shouldAutoScroll` rule, at a 50 point
threshold. Sending puts it back to the bottom. A rule that always followed would make a long answer
impossible to read while it was being written, and one that never followed would make streaming
pointless.

### 4.5 What the empty pane says

The heading, the line under it and four starter chips, from `ChatPage.tsx`'s own `STARTER_PROMPTS`
adapted to what Quill is: **Explain this file**, **Find the bug**, **Write a test**, **Summarise the
diff**. A chip fills the prompt rather than sending it, which is what the page it comes from does.

---

## 5. Streaming, and what it costs

The body of the message being streamed changes on every chunk, and rendering markdown is a parse, a
style span per run and a layout line per wrapped line. Rendering it once per chunk would be the
whole cost of the feature.

Two things bound it, and both are rules that already exist in Quill:

- **Nothing is rendered more than once a frame.** `markdown_text::Cache` re-renders only when the
  source or the width has changed, and the source is compared rather than a change being reported —
  `task-1666`'s rule, that a fingerprint derived from the state beats a list of the places that have
  to remember to say "I changed this". Chunks arrive faster than frames, so the ceiling is one
  render of one message per frame.
- **Only the streaming message is re-rendered.** Every finished message keeps its `Rendered` under
  its own key, so a conversation of forty messages costs nothing while the forty-first is arriving.

`cargo run --release -p quill-app --example chat_cost` measures it: a message of a given length,
rendered and laid out at the pane's width, and a whole pane's worth of decoration recorded. §11 has
the numbers. It is an example rather than a test, for the reason `frame_cost` is: a threshold in
milliseconds would be a different number on every machine.

The decoration is bounded the same way the board's is. `Canvases::texture_for` compares the kept
`Decor` list, the canvas rectangle and the scale, and rasterises nothing on a frame where none of
them moved — so a pane sitting still costs 0.001 ms, and a pane with an answer arriving costs one
rasterisation a frame of a **420 point column**, which is a quarter of the area a full board is.
`MAX_SCALE` caps the pixmap at 1.5 pixels a point, as it does everywhere else.

---

## 6. Pictures

Two directions, and they are different features that share a decoder.

**Into a message.** A picture is attached to the message being composed — from the `+` in the
prompt well (which opens `rfd`, the same picker `File -> Open` uses), by pasting one from the
clipboard, or by dragging a file onto the pane. It is shown as a thumbnail above the prompt, with a
cross to take it off again, and it goes up in the request: `image_url` with a `data:` URL for the
OpenAI shape, an `image` content block with `source.type = base64` for the Anthropic one. Both are
the same bytes with a different envelope, which is `wire.rs`'s job.

**Out of one.** A message that carries a picture draws it under its text, at the width of the bubble
and capped at 240 points tall, through `services::picture::upload` — which shrinks it to the card's
largest texture first, because egui *panics* when handed a bigger one and a four-thousand pixel
screenshot is an ordinary thing to paste into a chat. That is the same function the picture tab and
the Markdown preview already go through, and it is the reason a picture is never uploaded twice: the
texture is keyed on the attachment's id.

**A picture is bytes, not a path.** A conversation is saved with its pictures base64 in it, so a
conversation reopened after the file it came from has moved still shows what was sent. `base64.rs`
is written by hand, forty lines and a test that round-trips every length from zero to a thousand,
rather than a dependency: it is the same decision `services::control` made about JSON, and the
alternative was a crate for an alphabet.

**Nothing is fetched to draw one.** A message body naming `https://…` in an image mark shows its alt
text. The rule that a document cannot make a network request is not weakened by a pane that can.

---

## 7. Tool calls

The ticket says to understand them, and understanding them turns out to answer a bigger question.

### 7.1 What the page being copied does, and why Quill's answer is different

The ai-service chat's `StatusTopicEl` is a **report**: its server ran a tool and streamed progress
about it, and the client draws a tree of topics with timings. Quill has no server. A model talking
to `api.anthropic.com` emits `tool_use` blocks and then *waits* for the client to run them.

So there are two honest positions and no third. Either Quill shows the tool call and answers
nothing — in which case every conversation with tools switched on hangs on the first one — or Quill
runs it. Showing a tool call it will never answer is the worse of the two, because it looks like a
feature and behaves like a bug.

### 7.2 The tools are Quill's own commands, and that is the point of the product

`CLAUDE.md`'s first section is not a suggestion: *everything a person can do in this window, an
agent can do too, through the same command*. There are already a hundred and forty-seven of those
commands in `quill-cli/src/catalogue.rs`, already offered as MCP tools generated from that
catalogue, already tested. A chat pane inside the editor that could not reach them would be the one
agent-facing surface in Quill that is not agent-facing.

So the tools offered are the catalogue's, generated the way `mcp::tools` generates them — never
written out by hand, which is the rule that section states twice.

**It is off by default**, and it has the precedent the page being copied already set: that page's
own composer has a robot button whose title is *"UI controls (let the agent operate the studio)"*,
off unless pressed. Here it is the same button in the same place, plus `chat.tools` in the Settings
page, and the two agree because the button writes the setting.

### 7.3 How a provider runs one without becoming a second window

A provider cannot reach `QuillApp`. It draws, and it returns `Request`s that the window acts on once
the pane has been drawn — the rule `components::activity_bar` set and everything since has kept.

Two things are added, one in each direction:

```rust
Request::RunCommand { id: String, line: String }   // provider -> window
fn answered(&mut self, id: &str, answer: Result<serde_json::Value, String>)  // window -> provider
```

The window parses `line` against the catalogue exactly as `quill-cli` does, runs it through
`QuillApp::run_cli` — **the one place a command turns into a change**, so a tool call and a person
pressing the same menu entry are the same thing — and hands the answer back on the next frame. A
command that *waits* (`Outcome::Hold`) is refused with a sentence naming it rather than held: a tool
call that never returned would wedge the conversation, and the commands that wait are the ones an
agent should not be waiting on inside a chat turn.

The answer becomes a tool-result message, the conversation is sent again, and the model carries on.
`chat.tool_limit` bounds the number of round trips in one turn — eight — because a model that
decides to list every file in a loop should stop being funded by a pane nobody is watching.

### 7.4 What a tool call looks like

The `StatusTopicEl` block, in Quill's palette: a `Chrome::sunken` well, a raised round disc holding
`icon::terminal` for a command, `icon::run` while it is running, `icon::tick` when it worked and
`icon::cross` when it did not; the command's own name; the elapsed time; and, behind a caret, the
arguments and the answer as mono rows. Collapsed once it has finished, open while it is running,
which is `StatusTopicEl`'s own rule (`isTopicOpen`).

---

## 8. The Settings page

`Settings -> Agent-Chat`, contributed by `settings.page` in the manifest and drawn with
`components::modal`'s own rows and fields, inside the scrolling area
`components/agent_tasks/settings_page.rs` already established for a page with more than 640 points
in it.

### 8.1 Providers are rows, not a constant

The ticket asks for "a config in settings to allow configure url for Claude, codex etc." — so a
provider is a **list**, and the three that ship are just the three rows that are there the first
time it is opened:

| Name | Wire | URL | Model | Key |
|---|---|---|---|---|
| `claude` | anthropic | `https://api.anthropic.com/v1/messages` | `claude-opus-5` | `ANTHROPIC_API_KEY` |
| `codex` | openai | `https://api.openai.com/v1/chat/completions` | `gpt-5-codex` | `OPENAI_API_KEY` |
| `local` | openai | `http://127.0.0.1:8080/v1/chat/completions` | (whatever is loaded) | none |

Every field of every row is editable, rows can be added and removed, and one is the default. The
`local` row is there because the OpenAI shape is what llama.cpp, LM Studio, Ollama and every
gateway on this machine already speak, so "etc." is answered by a row rather than by a third
protocol.

**Two wire shapes and no more.** A third — Gemini's — was weighed and left out: it is a different
envelope again, nothing on this machine speaks it, and `wire.rs` is written so that adding one is a
match arm and a test rather than a redesign. `plugin.limitations` names it as absent rather than
letting a URL fail obscurely, and a provider naming a wire this version has not got is refused with
the list, which is the rule `language.renders`, `run.project`, `debug.adapter` and `ui.chrome` all
keep.

### 8.2 The rest of the page

- `chat.stream` — on. Off sends `stream: false` and shows the answer whole, which is what a proxy
  that will not stream needs.
- `chat.tools` — off, and what the composer's button writes.
- `chat.tool_limit` — eight.
- `chat.system` — a system prompt, empty by default. Quill sends its own line ahead of it saying
  which project is open and which file is showing, because a chat in an editor that does not know
  what you are looking at is a browser tab.
- `chat.history` — how many conversations to keep, twenty.
- Every row has a `Copy` button, which is what the Agent-Tasks page and the MCP page already do,
  because a page that shows a URL and gives no way to take it is a page somebody retypes a URL out
  of.

---

## 9. What an agent can do with it

The plugin machinery already gives every command to the command line and to MCP with no new
catalogue rows: `quill-cli plugins run agent-chat <command> …` and
`quill-cli plugins view agent-chat --json`, and `plugins show agent-chat` lists them. That is how
Agent-Tasks' twenty-odd commands are reached and it is the path `QuillApp::run_cli` already owns.

| Command | What it does |
|---|---|
| `new` | start a conversation |
| `send <text…>` | add a message and start the answer; answers at once with the message's id |
| `stop` | stop the answer |
| `messages` | the conversation as data: every message, its role, its text, its tool calls |
| `last` | just the last answer, for a caller that wants the sentence rather than the transcript |
| `state` | idle, sending, streaming, failed — and how many characters have arrived |
| `attach <path>` | attach a picture to the message being composed |
| `providers` | the configured endpoints, with their URLs and models and whether a key is present |
| `use <name>` | choose one |
| `history` | the conversations kept, newest first |
| `open <id>` | open one |
| `tools <on\|off>` | switch the catalogue tools on or off |

**`send` does not wait**, and that is deliberate rather than a shortcut. `UiProvider::command` is
called inside a frame; a command that blocked would stop the window drawing for the length of a
model's answer, which on a long one looks exactly like a crash — the sentence `quill-git`'s worker
exists for. So `send` starts it and `state` says whether it has finished, which is the shape
`run start` and `run output` already have.

`view()` is not optional and answers the whole pane as data: the provider, the state, the
conversation, the tool calls and the usage. A screenshot cannot answer "did it call the right tool",
and this can.

---

## 10. Tests

Four layers, as everywhere else.

1. **`quill-chat`, with no window.** The SSE reader fed the same stream split at every byte
   boundary. Both wire shapes built from a conversation and compared to the exact JSON the API
   documents. Both parsed from recorded event streams — a plain answer, an answer with a tool call,
   an answer with two tool calls interleaved with text, a refusal, a stream cut off mid-message.
   The state machine driven by hand-built `Reply`s. Base64 round-tripped at every length.
   A **scripted server**: a `TcpListener` on `127.0.0.1:0` that replays fixed bytes, which is
   `quill-dap`'s scripted-adapter arrangement with a socket instead of a pipe, and it is what makes
   "the whole client, end to end" a unit test.
2. **`quill-app`, with no window.** The provider's commands, its settings round-tripping through
   the plugin folder, the conversation store, the tool loop's limit, and `view()` answering what the
   drawing draws.
3. **Screenshot tests.** The pane empty; a conversation with a message each way; one with a code
   block, a table and a picture; one mid-stream; one with a tool call open and one finished; one
   failed; the Settings page. Built through `builder()` in `tests/screenshots.rs` so they share the
   graphics device pool, and `draw_deterministically` so `vello_cpu` uses `Level::baseline()`. Every
   one of them uses a **scripted server**, never a real endpoint: when a model answers is not
   something a test can know, which is the terminal's own rule.
4. **The real thing.** A real request to the local endpoint on this machine, and to
   `api.anthropic.com` with a real key, driven through `quill-cli plugins run agent-chat send` and
   read back through `plugins view`. §11 records what that run said.

---

## 11. What was measured

The numbers live here rather than in a comment so a later change can be compared against them.
`cargo run --release -p quill-app --example chat_cost -- [messages] [width] [height]` is how the
frame ones are measured again, and it is an example rather than a test for the reason `frame_cost`
and `vello_cost` are: a threshold in milliseconds would be a different number on every machine.

### 11.1 The dependency

| Route | Crates in the tree | Root store |
|---|---|---|
| `ureq` + `native-tls` | **31**, of which `schannel`, `windows-sys`, `serde_json`, `percent-encoding`, `http` and `base64` were already there | the machine's |
| `ureq` + `rustls` + `platform-verifier` | 121 | its own, plus the platform verifier |

### 11.2 A frame, on a 404 by 660 point pane

| What | Cost |
|---|---|
| Rendering and laying out one 504 byte answer | 0.136 ms |
| Asking for one that has not changed | 0.000 ms |
| A whole frame of forty messages with the last one arriving | 0.031 ms |
| Recording the decoration, which every frame pays | 0.001 ms |
| Asking for the decoration on a frame where nothing moved | 0.009 ms |
| Rasterising it on a frame where it changed, six bubbles showing | **8.7 ms** |
| The same with one bubble showing | 4.0 ms |

**The text is free and the decoration is not**, which is the opposite of what was expected. A
conversation of forty messages costs 0.031 ms a frame because thirty-nine of them are cached and the
fortieth is the only one rendered — `markdown_text::Cache`'s own rule, and the reason the cache is
keyed on the message rather than on the pane.

**The streaming case is the one that pays, and it is new.** `task-1765` measured a changed frame at
20.7 ms on a full board and revised its budget on the grounds that a changed frame happens *while
something is moving* — a drag, which lasts a second. Here the thing that moves is an answer arriving,
which lasts a minute: the bubble grows, every row below it moves, and the whole pane is rasterised
again. 8.7 ms is inside a 60 Hz frame and it is a lot of processor for a minute. What is done about
it is what §9.4 of that design already established: only rows that intersect the clip rectangle
record anything, `MAX_SCALE` caps the pixmap at 1.5 pixels a point, and
`Settings -> Appearance` has a tick box that turns the whole thing off at no cost at all. The sprite
cache §9.5 points at would help here more than it would on a board.

### 11.3 The round trip

Driven through the **released binary** against a real OpenAI-compatible server on loopback that
writes out the body it was sent, with `quill-cli plugins run agent-chat` on the other end. The pane
showed on the right, a chunked SSE answer streamed back a token at a time, `state` went
`sending` → `finished`, and `last` read the answer. With the tools on, the model asked for
`quill_tab list`, the window ran it through `run_cli`, the real answer came back into the
conversation, and the turn stopped at the eight round limit with the sentence that says so.

**It found three things, and all three were faults rather than surprises**: `ureq`'s default TLS
provider is Rustls whether or not that feature is compiled in, so an `https` request *panicked inside
the transport*; a failed turn came back on the wire as an empty assistant message; and a row scrolled
out of the conversation painted over the pane's own header, because `Ui::set_clip_rect` sets rather
than intersects and a `Chrome` records into a canvas that covers the whole pane.

---

## 12. Deliberately not here

Each with its reason, so that a later ticket can pick one up knowing what it is picking up.

- **More than one conversation at a time.** One pane, one conversation, a history to switch between
  them. Two at once is a tab strip inside a pane, which is a different feature.
- **Editing a message and re-sending from there.** The conversation is append-only. Branching a
  conversation is a tree, and a tree needs a way to see it.
- **Attaching a file's text automatically.** Quill sends which project is open and which file is
  showing; it does not send the file. A pane that quietly uploaded whatever was on the screen is a
  pane nobody could use on anything confidential. `@`-mentioning a file to include it deliberately
  is the right shape and is its own ticket.
- **Audio, speech and the microphone.** Four of the buttons on the page being copied are a
  text-to-speech service and a speech-to-text service that Quill has not got. A button that cannot
  apply is absent.
- **Web search and the datasource chips.** Both are that server's own features.
- **Gemini's wire shape**, per §8.1.
- **Retrying automatically on a 429.** The server says how long to wait and the pane says so; a
  client that retried on its own would be a client billing somebody's account while they were not
  looking.
- **A conversation shared between two Quill windows.** A Quill window is a process, and the
  conversations are files in the person's own folder read when the pane opens. Two windows chatting
  into one conversation is the same problem `session.txt` has and is not worth it for this.
