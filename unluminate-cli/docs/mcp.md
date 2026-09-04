# Unluminate's MCP server

Unluminate speaks the [Model Context Protocol](https://modelcontextprotocol.io), so an AI agent can
discover what the editor does and then do it — open files, read and change the text, run commands in
its terminals, search the project, drive its dialogs, work the Git menu, and take a screenshot of the
real window and look at it.

If you want to drive Unluminate yourself, read [commands.md](commands.md) instead. This is the same set of
commands with an agent's tool-calling machinery in front of it.

## The quickest way in

`Unluminate -> Edit -> Settings -> Tools -> MCP`, and press **Install for Claude Code** or **Install for
Codex**. Restart the agent. In Claude Code, `/mcp` will list Unluminate.

The same thing from a terminal:

```sh
unluminate-cli mcp install both
unluminate-cli mcp install claude --scope project   # this folder only, in .mcp.json
unluminate-cli mcp install codex --remove           # take it out again
```

It writes through the agent's own command — `claude mcp add-json`, `codex mcp add` — when that is on
the path, and edits the file directly when it is not. The direct edit takes a copy first, keeps
everything else in the file, and leaves the comments in Codex's TOML alone.

`codex mcp add` rewrites the whole of `config.toml` as it goes: the keys inside a table come back in
a different order, arrays are reflowed and `120` is written back as `120.0`. That is Codex doing what
it does whenever anybody runs the command, not something Unluminate asked for.

For a client with no button of its own, `unluminate-cli mcp config` prints the block to paste. It is the
same block the Settings page shows.

## What an agent is given

**The tools are generated from `catalogue.rs`** — the one list of commands that `unluminate-cli` parses
against and the window dispatches on. A command added to Unluminate is a tool the day it is added, and a
test fails if one ever is not. Nothing is written out by hand and there is nothing to keep in step.

There are two shapes, and `mcp.tools` chooses:

| `mcp.tools` | | |
|---|---|---|
| `grouped` | **the default** | One tool an area — `unluminate_tab`, `unluminate_editor`, `unluminate_git` — plus narrow generated aliases for semantic definition, references, and rename. Area verbs are an `enum`; every usage line and summary stays in the description. |
| `every` | | One tool a command — `unluminate_tab_open` — with every argument and flag a typed property. |

`grouped` costs an agent less than half the context `every` does, on every conversation the server
is connected to, and still names every command Unluminate has. The three aliases make the native semantic
answer as specific as a generic grep or edit tool without removing the compatible `unluminate_editor`
entry. `every` is there because a client
that permits tools by name — Claude Code does — can only say "may open tabs, may not quit" if
`tab open` is a tool of its own.

`unluminate-cli mcp tools --count` prints the real figures for both, against the catalogue as it is now:

```sh
$ unluminate-cli mcp tools --count
grouped     24 tools    67367 bytes   16841 tokens (roughly)
every      164 tools   164980 bytes   41245 tokens (roughly)
```

Argument names are generated from the catalogue. Kebab-case is canonical, but the MCP resolver
also accepts equivalent camelCase and snake_case spellings, such as `wait-for`, `waitFor`, and
`wait_for`. The control channel applies the same normalization. The common file guesses
`editor open`, `editor reload`, `editor save`, and `editor close` resolve to the corresponding
`tab` commands.

Two properties are on **every** tool in both shapes, because both are about the call rather than
about the command:

- **`instance`** — which running Unluminate to drive. See the next section.
- **`timeout`** — how long to wait for an answer, in milliseconds, 15000 by default. Raise it for
  something slow and lower it to fail fast; a command that waits for something of its own (the ones
  whose usage line carries a `--timeout`) waits for that and the call outlasts it. A call that does
  time out says what the window was doing — how long since it last drew a frame and how many
  requests are queued — so "it is busy" and "it has stopped" can be told apart, and it says whether
  the command was run. It usually was not: a request the caller gave up on is thrown away rather
  than applied later, so a timeout is safe to retry.

Two more things an agent gets:

- **A screenshot comes back as a picture.** `window screenshot` writes the PNG and the reply carries
  it as an image, so the agent can look at what a command did rather than be told where the file
  went. It is the only command special-cased, and it is a rule about the reply, so it holds in both
  shapes.
- **The whole written reference as a resource**, at `unluminate://commands.md`. It costs nothing until it
  is read, and it cannot go stale: a test fails while it disagrees with the catalogue.

## Which window a call goes to

A project is a window — `File -> New Window` starts a second process — so several Unluminates may be
running. In order, first answer wins:

1. `instance` on the tool call, `--instance` on `mcp serve`, or `UNLUMINATE_INSTANCE` in the environment.
   A process id, a port, or any part of a project's path.
2. Exactly one Unluminate running: that one.
3. Exactly one running Unluminate on the folder the server prefers — `CLAUDE_PROJECT_DIR`, which Claude
   Code sets in a spawned server's environment, or wherever the server was started, or the window's
   own project when a window is hosting it.
4. Otherwise a refusal that lists them. Guessing which window somebody meant is a command landing in
   the wrong project.

## The two transports

### stdio — what an agent launches

```sh
unluminate-cli mcp serve
```

The client starts it as a subprocess and speaks newline-delimited JSON-RPC over its pipes. Nothing
listens on a port and the process lives exactly as long as the conversation. **This is what the
install buttons write and what should be preferred.**

### Streamable HTTP — a URL instead

```sh
unluminate-cli mcp serve --transport http --port 7345
```

…or tick `Also serve over HTTP on this machine` in `Settings -> Tools -> MCP`, which makes the
running Unluminate host it. Either way the endpoint is `http://127.0.0.1:7345/mcp`.

- `POST /mcp` — a JSON-RPC request, answered `application/json`. A notification is `202` with no body.
- `GET /mcp` — `405`. There is no server-initiated stream; Unluminate has nothing to push.
- `DELETE /mcp` — `405`. There are no sessions.

One server drives every window, because it finds Unluminates through the instance files rather than
through its own process. So two Unluminates and one `mcp.port` is not a collision to fix: the second
window sees the port is held, does not start a second listener, and says so on the page.

## Which revision of the protocol

Both of the live ones, with one code path. `2025-06-18` is what clients speak today; `2026-07-28`
removed the `initialize` handshake and the protocol-level session altogether. This server therefore
**never requires `initialize`**, issues no session id, and echoes back whatever protocol version the
client named. `tools/list` and `tools/call` are answered the same whether a handshake happened or
not.

## Security

It is bound to `127.0.0.1` and never to anything else, with a test, exactly as the control channel
underneath it is.

The thing a local port genuinely has to defend against on a desktop is **a page in a browser**, which
can post to loopback. The control channel answers that with a token in a file, because a page can
post and cannot read a file. The HTTP endpoint cannot use a token — the configuration an agent copies
would have to carry it — so it uses the defence the specification mandates for exactly this attack: a
request whose `Origin` is not loopback is refused `403`, and so is one whose `Sec-Fetch-Site` says it
came from another site. A page cannot set either header.

**What none of this defends against is another program running as you.** Nothing on a desktop does.
Anything that can reach the HTTP port can `terminal send`, which is to say run a shell command in your
project. That is why `mcp.enabled` is **off** until you turn it on, and why stdio — where there is no
port at all — is the recommended way in.

## The settings

| Name | Accepts | Default |
|---|---|---|
| `mcp.enabled` | true or false | **false** |
| `mcp.port` | 1024 to 65535 | 7345 |
| `mcp.tools` | grouped or every | grouped |

The same names in `settings.conf`, on the Settings page, and in `unluminate-cli settings set`. A change
takes effect at once: the listener starts, stops or moves in the same frame.

`unluminate-cli mcp status --json` says what a window is actually doing, which is not always what the
settings say — the port may be held by another Unluminate, and an Unluminate started with `--control off` has no
command channel for a server to drive.

## Writing a client of your own

The MCP server is a client of Unluminate's own control channel, which is a smaller and older thing: a
loopback socket, one JSON object a line, a token in a file. If you are writing a program rather than
connecting an agent, [protocol.md](protocol.md) is three lines of Python and is probably what you
want.
