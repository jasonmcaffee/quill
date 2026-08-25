# quill-cli

Drive a running Quill window from the command line.

```sh
quill-cli launch .                                  # start a Quill on this folder and wait for it
quill-cli status --json                             # where am I?
quill-cli tab open README.md                        # open a file
quill-cli editor view preview                       # look at its Markdown preview
quill-cli terminal send cargo test                  # run something in the terminal
quill-cli terminal read --wait-for "test result"    # wait for it, and read what it said
quill-cli window screenshot _agent_output/quill.png # a real picture of the window
```

## What is in this folder

| | |
|---|---|
| `src/` | The client, and the library it shares with the window: the catalogue of commands, the wire format, and where a running Quill advertises itself. |
| `docs/commands.md` | **The reference.** Written to be handed to an AI agent whole. |
| `docs/protocol.md` | The socket protocol underneath, for writing a client in another language. |
| `agent-assessment/` | How well a local model does with the documentation, measured rather than assumed. |
| `examples/reference.rs` | Regenerates the reference half of `docs/commands.md` from the catalogue. |

## Building it

It is part of the Quill workspace, so it is built with everything else:

```sh
cargo build --release            # target/release/quill.exe and target/release/quill-cli.exe
cargo run -p quill-cli -- --help
```

Put `quill-cli` on the path, or run it from `target/release`. It looks for the `quill` program beside
itself, which is where a build and an installation both put it; `QUILL_BIN` names it somewhere else.

## How it works, in one paragraph

A running Quill listens on `127.0.0.1` on a port the operating system chose, and writes that port and
a per-run token into a small file in its settings folder. `quill-cli` reads that file, sends one JSON
object, and reads one back. The window answers at the top of its next frame, which is why a
screenshot taken straight after a command shows what the command did. Several Quills can run at once
— a project is a window — so `quill-cli instances` lists them and `--instance` picks one.

The channel is open by default and is closed with `quill --control off` or `QUILL_CONTROL=off`.
`docs/protocol.md` says what it is and why it is safe.

## The rule this exists to keep

Every feature Quill grows should be reachable from here, and should be documented. Both are enforced
by tests rather than remembered:

- **Every entry on every menu already is.** `quill-cli action list` is built by walking the real
  menus, and a test fails if a menu entry has no name — so a menu entry added tomorrow can be run
  from the command line tomorrow, with nobody adding anything.
- **Anything with no menu entry** is a row in `src/catalogue.rs` and an arm in
  `crates/quill-app/src/app/cli.rs`. The client parses against the catalogue and the window
  dispatches on it, so the two cannot come to disagree.
- **Documentation is a test.** `src/documentation.rs` fails while a command has no section in
  `docs/commands.md`, while a section's usage line is out of date, or while a section describes a
  command that no longer exists. `cargo run -p quill-cli --example reference` writes it.
