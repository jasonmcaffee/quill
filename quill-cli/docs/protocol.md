# The control channel

`quill-cli` is the comfortable way to drive Quill, not the only one. Underneath it is a small open
protocol: a socket on the loopback interface, one JSON object a line, a request and then a reply.
Anything with a socket and a JSON library can speak it in a few lines.

This document is what you need to write a second client. If you only want to run commands, read
[commands.md](commands.md) instead.

## Finding a running Quill

A Quill that is listening writes one file into an `instances` folder inside the folder it keeps its
settings in:

| Platform | Folder |
|---|---|
| Windows | `%APPDATA%\Quill\instances` |
| macOS | `~/Library/Application Support/Quill/instances` |
| Everywhere else | `$XDG_CONFIG_HOME/quill/instances`, or `~/.config/quill/instances` |

`QUILL_INSTANCES` names another folder, which is what the tests use.

Each file is named after the process id and holds `name = value` lines:

```
# A running Quill. Written by Quill, removed when it stops.
folder = C:\jason\dev\quill
pid = 24196
port = 51234
started = 1756139112
token = 4f1a9c2e77b3d051a8e6b40cf1927d3a
```

A file whose port nothing answers on is a Quill that was killed rather than closed. `quill-cli`
removes such a file when it finds one; a client of your own should treat it as no instance.

There is a file per window because **a project is a window** — `File -> New Window` starts a second
process — so there is no single fixed port to connect to.

## The conversation

Connect to `127.0.0.1` on the port in the file, write one line, read one line, close.

```
-> {"token":"4f1a…","command":"tab.open","arguments":{"path":"README.md"}}
<- {"ok":true,"command":"tab.open","message":"Opened C:\\jason\\dev\\quill\\README.md in tab 1","result":{"tab":1,"path":"…","picture":false}}
```

A refusal is the same shape with `ok` false:

```
<- {"ok":false,"command":"tab.open","error":{"code":"not-found","message":"There is no file at …"}}
```

### The request

| Field | |
|---|---|
| `token` | Required. The token from the instance file. A request without the right one is refused with `refused`. |
| `command` | Required. The command's **wire name**, with a dot: `tab.open`, `terminal.send`, `status`. |
| `arguments` | Optional. An object holding every value the command takes, positional or flag, under the name the catalogue gives it. An absent `arguments` is an empty one. |

Values may be sent as strings or as their natural type: `{"line": 42}` and `{"line": "42"}` mean the
same thing, because the CLI sends what somebody typed and a program sends what it has. A switch is
`true`.

`quill-cli commands --json` prints every command's wire name, arguments and flags, which is the list
to generate a client from.

### The reply

| Field | |
|---|---|
| `ok` | Whether it worked. |
| `command` | The command it is about. |
| `message` | A sentence for a person. Written by the window, because the window is the only thing that knows what actually happened. |
| `result` | The data. `null` for a command that only did something. |
| `error` | Present only when `ok` is false: `{"code":"…","message":"…"}`. |

Two keys in `result` have a meaning of their own, and a client may use them or ignore them:
`text` is content that is text all through — a document, a terminal screen — and `lines` is a
listing already laid out one row a line. `quill-cli` prints those and nothing else.

### The codes

| Code | Meaning |
|---|---|
| `not-found` | What was asked for is not there: a file, a tab, a setting, a result. |
| `not-applicable` | The command cannot apply to what is showing. |
| `usage` | The command exists but was given the wrong thing. |
| `unknown-command` | There is no command by that name. |
| `refused` | The token was missing or wrong. |
| `failed` | It was tried and it did not work. The message says why. |
| `timed-out` | It was still going when the time ran out. |
| `not-running` | Quill is closing, or could not be reached. |
| `several-instances` | Several Quills are running and none was named. |

## In Python, whole

```python
import json, socket, configparser, io, os, glob

def instances():
    folder = os.path.join(os.environ["APPDATA"], "Quill", "instances")  # see the table above
    for path in glob.glob(os.path.join(folder, "*.conf")):
        values = configparser.ConfigParser()
        values.read_string("[quill]\n" + io.open(path, encoding="utf-8").read())
        yield dict(values["quill"])

def ask(instance, command, **arguments):
    with socket.create_connection(("127.0.0.1", int(instance["port"])), timeout=30) as channel:
        channel.sendall((json.dumps({
            "token": instance["token"], "command": command, "arguments": arguments,
        }) + "\n").encode("utf-8"))
        return json.loads(channel.makefile("r", encoding="utf-8").readline())

quill = next(iter(instances()))
print(ask(quill, "status")["result"]["project"])
ask(quill, "tab.open", path="README.md")
ask(quill, "window.screenshot", file="_agent_output/quill.png")
```

## What it will and will not do

**It is bound to `127.0.0.1` and never to anything else.** Nothing off the machine can reach it, and
there is a test in `crates/quill-app/src/services/control.rs` that fails if that ever changes.

**The token is a capability, not a key.** It is sixteen bytes from the operating system's own
randomness, written into a file in the person's own settings folder — on a system with file modes,
mode `600`. It does not defend against another program running as that person; nothing on a desktop
does. What it does defend against is a page in a browser, which can post to a loopback port but
cannot read a file.

**The channel can be closed.** `quill --control off` starts a Quill that does not listen, and
`QUILL_CONTROL=off` in the environment does the same for every Quill started from that shell. It is
open by default, because a command line that has to be switched on first is a command line an agent
cannot rely on being there.

**One command at a time reaches the window.** The listener queues a request and the window answers it
at the top of its next frame, so a command's effect is in the frame about to be painted — which is
why a screenshot taken straight after a command shows what the command did. Four commands are
answered later than the frame they arrived on: `window screenshot`, `terminal read --wait-for`,
`modal results --wait` and `git action --wait`. Each has a timeout, so nothing waits for ever.
