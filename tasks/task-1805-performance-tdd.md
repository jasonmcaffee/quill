# task-1805 — making Unluminate cheaper to run

> *"How can we make Unluminate more performant without affecting features? Eg use less ram, less cpu,
> load faster. Deeply analyze and experiment. Research online. Write a TDD, then fully implement."*

Everything here was measured on the machine it was written on, before and after, with scripts kept in
`_agent_output/task-1805-performance/` so that any of it can be measured again. Nothing in it changes
what Unluminate does; every one of the 483 screenshot images is byte for byte what it was.

## 1. Where it started

Five runs, median, of the released 0.35.0 binary opening this repository and then being left alone:

| | 0.35.0 |
|---|---|
| process start until the window is **on the screen** | **1162 ms** |
| process start until the control channel **answers** | 1240 ms |
| working set | 222.6 MB |
| private bytes | 438 MB |
| **processor time while completely idle** | **43.4 ms/s** |
| `unluminate.exe` | 29.9 MB |
| threads | 41 |

The idle number is the one that stopped the room. **A window with nobody touching it was using a
twenty-third of a processor core, for ever, to show a picture that was not changing.** egui's own
README says the opposite is the design — *"egui only repaints when there is interaction… so if your
app is idle, no CPU is wasted"* — and Unluminate deliberately draws twice a second anyway, for a
reason `app::HEARTBEAT` records. Two frames a second at 43 ms/s is **21 ms a frame**, and egui's own
estimate for a whole application is 1–2.

## 2. The instrument that was missing, and why none of the existing ones could answer

There are already five cost examples — `frame_cost`, `completion_cost`, `symbol_cost`,
`folding_cost`, `vello_cost` — and every one of them said its own piece was cheap. Every one of them
was right. None of them is the window: they measure one component with no eframe, no wgpu, no
plugins, no explorer and no project behind it, and the number the operating system was reporting was
about the whole.

So two instruments were built, and both are meant to be kept.

**`services::frame_trace`** — `UNLUMINATE_FRAME_TRACE=<file> unluminate` writes one line per frame:

```text
frame 21.482 outside 458.926 | control 0.006 git 0.001 colour 0.001 menus 0.726 chrome 0.541 explorer 22.024 …
```

`frame` is `UnluminateApp::ui` end to end, `outside` is everything between two frames (egui's
tessellation, the graphics card, the wait), and each name after the bar is one phase. It also writes
`mark` lines during startup saying when the program had got to each step of `main`. **It costs one
relaxed atomic load when it is off**, which is the only way an instrument may live in a hot path.

**`examples/startup_cost.rs`** — the one step `main`'s own marks cannot see inside: it builds a
graphics instance, asks for an adapter, asks for a device and compiles egui's shader exactly as
`eframe` does. `--no-graphics` leaves that out, and the difference between the two runs is what the
graphics driver costs in time and in memory.

The first trace answered the question in one run:

```text
frame 21.497 | … menus 0.749 chrome 0.553 explorer 21.944 panes 0.078 tiles 0.063 rest 0.057
```

**The explorer was 22 of 24 milliseconds.** Bisecting inside it put 13.5–19.8 ms of that in one
place.

## 3. Four faults, each measured

### 3.1 The explorer's footer stat-ed every file in the project, on every frame

`FileTree::openable_count()` was

```rust
self.all_files.iter().filter(|path| is_openable(path)).count()
```

and `file_kind::openable` does a `std::fs::metadata` per file and then, for a name whose extension it
has never heard of, **opens the file and reads four kilobytes of it**. The footer that shows the
answer — *"917 files · 916 can be opened"* — is drawn every frame. So an idle window ran 917
`metadata` syscalls plus a set of file reads **twice a second**, to redraw one line of text that had
not changed; during any interaction, at sixty frames a second, that is about 55,000 syscalls a
second.

It is the same fault `task-28` found and fixed in `Entry::new`, whose comment says in as many words
that this function *"does its own `metadata` call and then, for a name with an unknown extension,
**opens the file and reads it**. That is what froze the window on a large folder and hung on
`/dev`"* — left behind in the one caller that ran it over the whole project rather than over one row.

The count is now worked out **once, during the walk that already built the file list**, from the
`metadata` call `read_directory` had already made. `walk_files` answers with a `Walked { files,
openable }` and the number costs nothing.

It also settles a disagreement nobody had noticed. The rows are drawn from
`file_kind::openable_in_a_listing`, which offers a name it has never heard of; the footer asked
`file_kind::openable`, which reads the file and refuses it if the bytes are not text. So a `.bin`
full of text was drawn as an ordinary, openable row and left out of the count underneath it. A footer
counts rows, and rows are drawing, so the drawing rule is the right one — which is what `file_kind`'s
own documentation already said.

**Idle frame 24 ms → 1.4 ms. Idle processor time 43.4 ms/s → about 5.**

### 3.2 A window that was killed cost 414 ms of the next one's startup — and of every CLI command

`main` called `unluminate_cli::client::running()` on every start, and that dials every listed
instance's port with a 400 ms timeout. Two things were wrong with it.

**It asked the port when the question was about the process.** A window that is killed rather than
closed leaves its instance file behind, and Unluminate is killed rather than closed all the time — a
task manager, a crash, a script. Measured on this machine, a dead loopback port **does not answer
with a refusal**; it answers with nothing, so a single stale file spent the entire timeout.
`instances::is_running` asks the operating system first, which takes microseconds, and the port is
dialled only when the process really is there — because a live process id can have been handed to
something else entirely and only the port can settle that.

**And it asked at all when there was nothing to filter.** The answer is used for one thing: skipping
projects that already have a window while a desktop launch restores a session. Every other start —
`unluminate .` in a terminal, `unluminate-cli launch`, `File → New Window`, a file opened from the
shell — names its folder, so the list is empty and the answer is thrown away. It is now asked only
when there is something to filter.

**A third fault, found by the harness after the first two were fixed.** Opening a handle is not
enough on Windows: the operating system keeps a process *object* alive for as long as anything holds
a handle to it — the parent shell, a task manager — so `OpenProcess` on a process that has been
killed still succeeds. The measurement was unmissable once the startup harness ran seven times in a
row: the first run, the only one with no just-killed window behind it, answered in 937 ms and every
later one in **1399**. `running_process` asks `GetExitCodeProcess` now, and a process that exits with
259 — the value Windows also uses for `STILL_ACTIVE` — falls through to the port probe, which is what
this replaced, so the ambiguity costs a little time in a rare case and can never give a wrong answer.

The same question was being answered twice in the tree: `services::agent_tasks` had its own copy of
`kill(pid, 0)` / `OpenProcess`. There is one now, in `unluminate-cli::instances`, beside the files
that ask it.

**`unluminate-cli status` with a stale instance file present: 431 ms → 75 ms.** That is the number
that matters most for an agent, because every `unluminate-cli` command pays it.

### 3.3 `Plugins::grammars()` deep-cloned every plugin's word lists, three times a frame

It built the whole extension→grammar set on every call, cloning each `Grammar` — its keywords, its
builtins, its types, its definers and its import words, several hundred `String`s for a language like
TypeScript — **once per extension that plugin claims**. Eleven plugins claiming two dozen extensions
between them made two dozen full copies a call.

`UnluminateApp::menu_state` calls it three times a frame, to answer three yes/no questions:
`definitions_apply_here`, `symbols_apply_here`, `completion_applies_here`. **0.43 ms of a 1.4 ms
frame** — the largest single thing left in an idle one, and a tax on every interactive frame too.

It is a field now, worked out by `Plugins::settle` whenever the plugins change; every function in
`plugins.rs` that changes which plugins there are or which of them are on calls it, and all of them
are in that file. `switching_a_plugin_off_changes_the_grammars_it_offers` is what pins the
invalidation, because the one thing a kept answer can do that a derived one cannot is go stale.

A thread still takes a **copy** and that has not changed: `services::symbol_index` and the reference
mode of `services::text_search` outlive the frame that started them, and a plugin switched off while
one is running must not change what it is half way through answering. They clone it themselves, which
is the caller that needs a snapshot paying for one.

**Idle frame 1.4 ms → 0.65 ms.**

### 3.4 Starting shells kept the window off the screen

eframe keeps the window hidden until it has painted once, so **everything `restore_project` does is
blank desktop**. On a project left with two terminals it was 179 ms of a 724 ms startup — and all 179
were the two shells: a pseudoconsole and a process each.

The tabs to open are written down now and started on the **second** frame. Not the first: work at the
end of the first frame is still before eframe shows the window, which was the whole point, and that
had to be measured to be believed — the first attempt put the call at the end of `ui()` and bought
nothing at all. The window asks to be drawn again at once, so the terminal tile is empty for one
frame rather than until `HEARTBEAT` next wakes it half a second later.

`pump_control` starts them too, because a command arriving before that second frame must not be
answered with a half-restored window — `terminal list` would say there were none. Taking the list is
what makes two call sites safe: whichever asks first does the work and the other finds nothing to do.

The size is, if anything, more honest than before: `terminal_grid_size` falls back to a guess until
the window has drawn a frame, and after one frame it has the real rectangle.

**Window on the screen: 727 ms → 574 ms.**

## 4. The memory is the graphics driver's, and that is measured rather than assumed

`examples/startup_cost.rs`, run both ways, watched from outside:

| | working set | private bytes |
|---|---|---|
| fonts + plugins + a walk of 917 files (`--no-graphics`) | **12.3 MB** | **6.4 MB** |
| the same, plus a DX12 device and egui's shader | **135.7 MB** | **315.8 MB** |
| the whole Unluminate window | 223 MB | 438 MB |

**The DX12 device alone is 123 MB of working set and 309 MB of private bytes** — 55% of the working
set and 70% of the private bytes of a running Unluminate, and none of it is Unluminate's to give
back. What is left, about 87 MB of working set, is the window, its swapchain, the glyph atlas, two
shells, the file tree, the plugins, the settings, the tabs and every piece of state the editor holds.

For scale: Zed, the fastest native editor there is, benchmarks at about **222 MB** and VS Code
between 1 and 3.5 GB. Unluminate is already where the good end of that range is, and the reason is
that it is a native window rather than a browser.

**So there is no memory work in this ticket, and that is a finding rather than an omission.** The
honest thing to do with a number nobody can move is to measure it, write down where it goes, and
leave it alone. §7 records the one place a real saving might still be found.

## 5. Where the remaining startup goes

`startup_cost` in the order eframe asks:

| | ms |
|---|---|
| graphics instance | 14 |
| **request adapter** | **362** |
| request device | 77 |
| compile egui's shader | 15 |
| font database (`fontdb`, 7 families offered) | 13 |
| plugins (11 read) | 0.7 |
| walk this project (917 files) | 13 |

And the marks from a real start of the finished build:

```text
mark arguments          0.000
mark crash-log          0.060
mark login-shell        0.126
mark recent-projects    0.615
mark running-instances  0.632
mark project-state      1.473
mark eframe-window    428.115      <- the window and the graphics device
mark fonts            448.946
mark plugins          449.881
mark file-tree        468.255
mark prepare          469.940
mark settings         474.844
mark restore-project  481.382
mark ready            483.064
```

**Everything Unluminate does before the graphics device is 1.5 ms, and everything after it is 55 ms.**
The remaining 428 is winit and a DX12 driver enumerating adapters and building a device, and it is the
floor for any window drawn on the graphics card. It was 1104 ms to `ready` when this ticket opened.

## 6. Weighed and rejected, with the numbers

**`lto = "fat"` with `codegen-units = 1`.** Takes `unluminate.exe` from 29.9 MB to **26.8** and
`unluminate-cli.exe` from 1.29 to 1.0 — and moves nothing anybody can feel: startup 738 ms against
734, a keystroke on a 380 KB file 2.498 ms against 2.508, working set 220.4 MB against 220.9. What it
costs is the build, **52 seconds to 4 minutes 25**, five times over, on a repository whose rule is
that finishing a task means running a release build. A tenth off the installer is worth having and it
is not worth that. The measurement is in `Cargo.toml` beside the setting so the trade can be made
again rather than argued.

**`strip = true`.** Refused. `services::crash_log` sets `RUST_BACKTRACE=1` itself, precisely because
*"the person whose Unluminate just disappeared did not set an environment variable before it
happened"*, and a stripped binary writes a crash log of hexadecimal addresses. The release profile
already carries no debug info, so there is little to strip anyway.

**`panic = "abort"`.** Refused, and for the same file's reason: the crash log is written from a panic
hook, and changing how a panic unwinds out of an event loop is not a thing to do for a few hundred
kilobytes.

**A longer `HEARTBEAT`.** Two frames a second at 0.65 ms is 1.3 ms/s of the 5.6 that is left, and a
one-second heartbeat would halve it. Refused: half a second is the promise `services::wake` and
`services::control` are both written against — *how long a window that has stopped answering takes to
recover by itself* — and weakening a stated guarantee to save a millisecond a second is a bad trade.

**Copying the file into a `String` on every keystroke.** `colour_the_file` does
`document.text().to_string()`, which on a 380 KB file is **0.23 ms** of the 1.27 ms a keystroke costs.
It could be avoided, but only by teaching `unluminate_core::incremental` to read a rope rather than a
`&str`, which is a change to the tokeniser's whole interface for a fifth of a millisecond. Written
down rather than done.

**Anything else about typing.** A keystroke on the largest file in this repository is **2.7 ms
median, 6.3 ms worst**, against a 16.7 ms frame at sixty a second. Broken down: applying the style
spans 0.61 ms, the incremental tokeniser 0.36, the rope copy 0.23, the fold tokens 0.08. `task-1666`
and `task-1804` already did this work and it holds; there is no cheap win left in it.

## 7. What is left, in the order it would be done

1. **Ask for a graphics device on a thread, or later.** 428 ms of a 584 ms startup is eframe building
   a DX12 device before anything is drawn. Nothing in Unluminate controls that ordering today —
   `eframe::run_native` owns it — so this is a change to eframe or a move off it, and it is by far
   the largest number left.
2. **The rope copy per keystroke**, §6. 0.23 ms, and an interface change.
3. **`menus` at 0.18 ms a frame.** The menu tree is rebuilt from the whole of the window's state on
   every frame; roughly 109 entries, each with an owned `String` name. Most of those names are
   constants and could be `Cow<'static, str>`. It is the largest phase left in an idle frame and it
   is a fifth of a millisecond.
4. **`notice_what_changed_on_disk` at 0.18 ms.** It asks the modification time of the root and every
   folder that is opened out, every 750 ms. Cheap, correct, and the alternative — `notify` — is a
   dependency, a thread, a channel and a debounce, which `task-1693` already refused for good reasons.

## 8. The numbers

Seven runs each, median, same script, same project, same machine, idle measured over a settled 45
seconds:

| | before | after | |
|---|---|---|---|
| window **on the screen** | 1162 ms | **584 ms** | **2.0x** |
| control channel **answering** | 1240 ms | **834 ms** | 1.5x |
| **processor time while idle** | 43.4 ms/s | **5.6 ms/s** | **7.8x** |
| **one idle frame** | ~24 ms | **0.65 ms** | **37x** |
| one keystroke, 380 KB file | 2.5 ms | 2.7 ms | unchanged |
| `unluminate-cli` with a stale instance file | 431 ms | **75 ms** | **5.7x** |
| working set | 222.6 MB | 223.1 MB | unchanged, and §4 says why |
| private bytes | 438 MB | 439.6 MB | unchanged, and §4 says why |
| `unluminate.exe` | 29.9 MB | 30.0 MB | unchanged |

957 unit tests, 483 screenshot tests and every other crate's suite pass, and not one accepted image
changed.

## 9. How to measure it again

```text
UNLUMINATE_FRAME_TRACE=trace.txt unluminate <folder>
cargo run --release -p unluminate-app --example startup_cost -- <folder> [--no-graphics]

pwsh _agent_output/task-1805-performance/measure.ps1     -Label after     # startup, memory, idle cpu
pwsh _agent_output/task-1805-performance/trace.ps1       -Label after     # the phases of an idle frame
pwsh _agent_output/task-1805-performance/interactive.ps1 -Label typing    # the phases of a keystroke
```

None of these is a test, for the reason `frame_cost` gives: a threshold in milliseconds would be a
different number on every machine. What *is* a test is the work itself — the openable count comes
from the walk, the grammars are invalidated when a plugin is switched off, and the terminals really
do come back.
