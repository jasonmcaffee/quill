# Unluminous — the product video

**task-1793**, re-shot by **task-1811**. A 4K demo of Unluminous for jasonmcaffee.com,
blackrainbowlabs.com and unluminous.com.

This is the script and the shot list. It is also the thing the recording harness reads: every beat
below names the exact `unluminous-cli` commands that produce it, so the video is *driven* rather than
hand-flown, and any beat can be re-recorded on its own without re-shooting the ones around it.

---

## 1. What the video has to say

Three claims, in this order, because the third only lands once the first two are believed:

1. **It is beautiful.** The desktop shows through the window; the text does not. Nothing else on
   this machine looks like it.
2. **It is a real IDE.** Folding, go-to-definition, project-wide rename, run configurations, a
   debugger with variables and watches, a database explorer, rendered HTML tabs, git, themes. Not a
   text editor with ambitions.
3. **It is AI-first, and that is a definition rather than a slogan.** Everything a person can do in
   the window, an agent can do too, through the same command. The video *proves* it on camera: a
   command line drives the live window, an agent chats beside the code, and a board of tickets is
   worked by agents in terminals Unluminous owns.

The arc is proof-then-payoff. The look opens it, the IDE depth earns the right to make the claim,
and the AI act is the crescendo. A viewer who stops at 90 seconds still saw a real editor; a viewer
who stays to the end saw the argument.

## 2. Format

| | |
|---|---|
| Resolution | 3840 x 2160 (native panel, no upscale) |
| Frame rate | 60 fps capture, 60 fps delivery |
| Length | ~4 min 10 s |
| Codec | H.264 (`h264_nvenc`, CQ 19, high profile) + AAC 192k, `+faststart` |
| Audio | one music bed, no voice-over |
| Composition | full-desktop capture: wallpaper, then Unluminous at **3296 x 1840**, placed at 272,160, at **0.87** background opacity |
| Type in the window | editor 41, UI 29, terminal 34 |
| Text | overlay cards rendered as HTML at 4K with alpha, composited in post |

**Why a margin and not a full-screen window.** The transparency is the thing to notice first, and it
is only legible against something. A window filling the screen has no edge, so the wallpaper reads as
Unluminous's own background rather than as the desktop behind it.

**Why the window is not as big as the panel (task-1811).** The first cut used 3696 x 2016 inside the
3840 x 2160 frame — a 72 px margin, a hair under 3.4% — and the type in it was hard to read. The font
size was part of that and is now 20% larger throughout, but the window was the bigger half: at 96% of
the frame, a glyph is a small fraction of what a viewer is looking at, and smaller again once a site
scales the video down to fit a column. 3296 x 1840 gives up 11% of the width and 9% of the height, so
the same glyph is about **1.35x** larger relative to the window it sits in, and the margin grows from
72 px to 272 x 160 — which happens to serve the first claim rather than fight it, because there is
now plainly more wallpaper to see through.

**Why no voice-over.** The overlay text says the sentence and the window proves it in the same
second. A narrator would have to say it slower than the window can show it.

## 3. The demo project

`fleet` — a small but real full-stack project written for this video and kept in the repo at
`_agent_output/task-1793-unluminous-video/fleet/`.

```
fleet/
  Cargo.toml
  README.md          Markdown + a Mermaid flowchart + a table   -> preview beat
  src/main.rs        the CLI entry point                        -> editing, run, debug
  src/telemetry.rs   the types: Drone, Flight, Incident, Health -> folding, definition, rename
  src/report.rs      the analysis                               -> breakpoints, variables, watches
  src/db.rs          the SQLite reader                          -> definition, references
  schema.sql         the schema                                 -> SQL colouring
  fleet.db           SQLite, 24 drones / 480 flights            -> database explorer
  web/index.html     a dashboard                                -> rendered browser tab
  web/app.ts         its script                                 -> TypeScript colouring
  web/style.css      its styles                                 -> CSS colouring
```

One project exercises **every** language plugin Unluminous ships (Rust, TypeScript, JavaScript, CSS, HTML,
SQL, Mermaid), both database engines' shapes, the debugger, the run tile, the browser tab and git —
without a single beat needing a different folder open. It is a git repository with two files
deliberately left modified, so the git beat has something true to show.

It is deliberately not a toy. The report code has nested structures worth stopping inside: a
breakpoint in `report::fleet_health` has a `HashMap<String, Vec<Flight>>` and a `Health` struct in
scope, which is what makes the variables tree worth looking at.

## 4. Beat sheet

Times are cumulative and are the target, not a promise; the edit trims to the beat, not to the clock.
Every beat is one recorded segment. **Every `unluminous-cli` line below is literal** and is what the
harness sends.

---

### Beat 0 — Cold open · 0:00 – 0:08

**On screen.** The wallpaper alone for 0.6 s. Unluminous fades up from nothing over 1.2 s, already open on
`fleet`, `src/telemetry.rs` showing, explorer down the left, no panels along the bottom. Nothing
moves for a beat; the viewer is allowed to look at it.

**Overlay.** Centre, large:

> **Unluminous**
> An AI-first IDE, written in Rust.

**Setup.**
```
unluminous-cli settings set appearance.background.opacity 0.87
unluminous-cli settings set appearance.font.size 41
unluminous-cli settings set appearance.ui.font.size 29
unluminous-cli settings set terminal.font.size 34
unluminous-cli window position --x 272 --y 160
unluminous-cli window size --width 3296 --height 1840
unluminous-cli tab open src/telemetry.rs
unluminous-cli panel reset
```

---

### Beat 1 — The window · 0:08 – 0:22

**On screen.** A slow scroll through `telemetry.rs`, then the explorer expands `web/`. The point is
the ground: every surface is translucent and the type is not.

**Overlay.** Lower left:

> The desktop shows through. The text stays solid.

**Script.**
```
unluminous-cli editor scroll --line 40           # slow, 8 lines a step
unluminous-cli explorer expand web
unluminous-cli editor scroll --line 1
```

---

### Beat 2 — Editing · 0:22 – 0:44

**On screen.** The caret goes to the end of `impl Drone`, and a method is typed in — character by
character, at a human 55 ms a keystroke, so it reads as typing rather than as paste. The completion
popup appears on `self.` and a row is chosen from it.

**Overlay.** Upper right:

> Completion reads the project, not a dictionary.

**Script.**
```
unluminous-cli editor caret --line 63 --column 1
unluminous-cli editor insert "…"                 # one character per call, 55 ms apart
unluminous-cli editor complete --stem dur        # shown, then chosen
unluminous-cli editor complete --stem dur --choose 0
unluminous-cli tab save
```

---

### Beat 3 — Navigation · 0:44 – 1:04

**On screen.** Three moves, quickly: **go to definition** jumps from a use of `Health` in
`report.rs` into `telemetry.rs`; **find references** opens the references pane with every use listed
and each one classified; **rename** changes `fleet_health` to `fleet_summary` across the project, and
the tabs that hold it all update at once.

**Overlay.** Three cards, one a move:

> Go to definition.
> Find every reference — and it knows which are code, which are comments.
> Rename across the project. One undo puts it all back.

**Script.**
```
unluminous-cli tab open src/report.rs
unluminous-cli editor definition --name Health
unluminous-cli editor navigate-back
unluminous-cli editor references --name Flight
unluminous-cli editor rename --name fleet_health --to fleet_summary
unluminous-cli editor undo
```

The undo at the end is not tidying — it is the shot. A project-wide rename that comes back in one
keystroke is the sentence "this is a real refactor, not a find-and-replace" said without words.

---

### Beat 4 — Folding · 1:04 – 1:14

**On screen.** Every block in `telemetry.rs` collapses at once, leaving the file's shape: five
structs and three impls on eight lines. Then one expands.

**Overlay.** Lower left:

> Collapse the file to its shape.

**Script.**
```
unluminous-cli tab open src/telemetry.rs
unluminous-cli fold collapse --all
unluminous-cli fold expand --line 12
```

---

### Beat 5 — Markdown and Mermaid · 1:14 – 1:32

**On screen.** `README.md` opens; the view goes to source-and-preview side by side. The preview holds
a laid-out Mermaid flowchart of the fleet pipeline, headings, a table. A line is edited in the source
and the preview follows it.

**Overlay.** Upper right:

> Markdown, side by side. Mermaid diagrams are laid out, not embedded.

**Script.**
```
unluminous-cli tab open README.md
unluminous-cli editor view split
unluminous-cli editor scroll --line 20
unluminous-cli editor caret --line 8 --column 1
unluminous-cli editor insert "…"                 # typed, so the preview updates live
```

---

### Beat 6 — A rendered page · 1:32 – 1:44

**On screen.** `web/index.html` opens as a **rendered tab** — the dashboard, styled, with its
TypeScript running — beside the source of the same file in the other pane.

**Overlay.** Lower right:

> HTML opens as a page. Its CSS, scripts and images resolve inside the project.

**Script.**
```
unluminous-cli pane split
unluminous-cli browser open web/index.html
```

---

### Beat 7 — Run configurations · 1:44 – 1:58

**On screen.** The run tile comes up along the bottom, `fleet report` is selected and started, and
the report prints into the tile in a pseudoterminal — colour and all.

**Overlay.** Upper left:

> Run configurations, per project. The output is a real terminal.

**Script.**
```
unluminous-cli run list
unluminous-cli run select "fleet report"
unluminous-cli run start
unluminous-cli run output
```

---

### Beat 8 — The debugger · 1:58 – 2:34

The longest beat, because it is the one people do not believe an editor written from scratch has.

**On screen, in order.** A breakpoint appears in the gutter at `report.rs:48`. `fleet report` starts
under the debugger. The program stops; the current line lights. The debug tile shows the call stack,
and the variables tree opens `flights` — a `HashMap` with keys, then a `Vec<Flight>` under one of
them, then a `Flight`'s own fields. A watch on `flights.len()` is added and answers. Step over twice;
the highlight moves and a value in the tree changes. Continue.

There is no hover in this beat, and task-1811 took the one it had out. `debug hover` is a value the
**command line** answers — the window paints no tooltip for it, and the capture draws no pointer — so
on camera it was a three-second pause in the middle of the longest beat with nothing to look at. The
variables tree shows the same values and shows them being opened.

**Overlay.** Three cards through the beat:

> A real debugger. Breakpoints, frames, variables.
> Walk the values while it is stopped — a map, a vector inside it, a struct inside that.
> Watch an expression. Step. Continue.

**Script.**
```
unluminous-cli debug breakpoint add src/report.rs 48
unluminous-cli debug start
unluminous-cli debug status
unluminous-cli debug frames
unluminous-cli debug variables --expand flights
unluminous-cli debug watch add "flights.len()"
unluminous-cli debug step-over
unluminous-cli debug step-over
unluminous-cli debug evaluate "health.uptime_ratio()"
unluminous-cli debug continue
```

---

### Beat 9 — Panels and panes · 2:34 – 2:50

**On screen.** The terminal is dragged from the bottom edge to the right; the four drop bands show
while it moves. It is resized. The editing area is split, a tab is dragged into the second pane, and
the panels are reset.

**Overlay.** Lower left:

> Every panel docks to any edge. Every pane splits.

**Script.**
```
unluminous-cli panel dock terminal --side right
unluminous-cli panel size terminal --width 900
unluminous-cli panel dock terminal --side bottom
unluminous-cli pane split
unluminous-cli tab move --pane 1
unluminous-cli pane width --fraction 0.62
unluminous-cli panel zoom explorer --factor 1.15
unluminous-cli panel reset
```

---

### Beat 10 — The database explorer · 2:50 – 3:08

**On screen.** The Database pane opens on the right: the `fleet.db` data source, its tables, a
table's columns. The Database tab opens a query console; a `select` is typed and run; the grid fills.
A cell is edited and the pending change is shown **as the statement it will become**, before anything
is written.

**Overlay.** Upper right:

> A database explorer. PostgreSQL and SQLite.
> A change is shown as the statement it will become, before it is written.

**Script.**
```
unluminous-cli plugins pane database/explorer --side right
unluminous-cli plugins tab database/workspace --open
unluminous-cli plugins run database console --sql "select tail, model, hours from drone order by hours desc limit 12"
```

---

### Beat 11 — Themes · 3:08 – 3:20

**On screen.** Five themes in five seconds — Islands Dracula Colorful, Material Palenight, Material
Deep Ocean, Monokai Pro, One Dark — the whole window and the code in it repainting each time, then
back to Unluminous Dark. Held on the last one for a beat.

**Overlay.** Centre bottom:

> Five themes, every colour read out of the IntelliJ plugin that ships it.

**Script.**
```
unluminous-cli theme set dracula
unluminous-cli theme set palenight
unluminous-cli theme set deep-ocean
unluminous-cli theme set monokai-pro
unluminous-cli theme set one-dark
unluminous-cli theme set unluminous-dark
```

---

### Beat 12 — The command line drives the window · 3:20 – 3:36

The hinge of the whole video, and the reason the AI act is credible rather than asserted.

**On screen.** Unluminous's own terminal, along the bottom. Commands are typed into it — and the window
above obeys them, live, in the same frame. A file opens. A pane splits. A breakpoint is set. A theme
changes. Nothing is clicked.

**Overlay.** Centre:

> This is Unluminous's own terminal, driving the window it is running in.
> Every menu entry, every key chord, every button — one command each.

**Script** (typed into the terminal tab, visible on camera):
```
unluminous-cli tab open src/db.rs
unluminous-cli fold collapse --all
unluminous-cli pane split
unluminous-cli theme set monokai-pro
unluminous-cli action list | head
```

---

### Beat 13 — Agent chat · 3:36 – 3:52

**On screen.** The Agent-Chat pane opens on the right. A question is typed — *"what does
fleet_health do, and where does its data come from?"* — and the answer streams in, a token at a time,
with the agent's own tool calls shown as it reads the project. The answer is about the code that is
on the screen.

**Overlay.** Lower right:

> The `claude` and `codex` already on your machine, beside your code.
> They bring their own tools and their own keys. Unluminous holds none.

**Script.**
```
unluminous-cli plugins pane agent-chat/chat --side right
unluminous-cli plugins run agent-chat send "what does fleet_health do, and where does its data come from?"
unluminous-cli plugins run agent-chat state --wait
```

---

### Beat 14 — Agent tasks, and the close · 3:52 – 4:05

**On screen.** The Agent-Tasks board opens as a tab: four lanes, cards with todos and comments. A
ticket opens as a modal. A card's play button starts an agent in a terminal Unluminous owns, and the
terminal appears with the agent working in it. Cut to the whole window, held.

**Overlay**, the last card, centre, held to black:

> Everything a person can do in this window,
> an agent can do too — through the same command,
> and both are covered by the same tests.
>
> **Unluminous**

**Script.**
```
unluminous-cli plugins tab agent-tasks/board --open
unluminous-cli plugins run agent-tasks open UNLUMINOUS-4
```

---

## 5. How it is recorded

**Capture.** `ffmpeg -f gdigrab -framerate 60 -i desktop` over the whole 3840x2160 panel, into
`h264_nvenc` at CQ 16 for the masters (the grade is done once, at the end, on the assembled cut). The
taskbar is set to auto-hide and desktop icons are hidden for the duration, and both are restored
afterwards — a taskbar in the 72 px margin would be the one thing in frame that is not the product.

**One segment per beat.** Each is recorded on its own, from a known window state, so a beat that goes
wrong costs one beat. `_agent_output/task-1793-unluminous-video/segments/beat-NN.mp4`.

**Driving.** `scripts/drive.mjs` sends the `unluminous-cli` lines above with the pauses written beside
them, so the pacing is in the script rather than in an operator's hands. Typing is one
`editor insert` per character at 55 ms; a scroll is stepped rather than jumped, because a jump reads
as a cut.

**Overlays.** Each card is an HTML page rendered headless at 3840x2160 with a transparent background
and screenshotted to PNG, then composited with ffmpeg `overlay` under a 12-frame fade. HTML rather
than `drawtext` because the cards need real typography — letter-spacing, two weights, a soft shadow
so they read over both dark code and a bright patch of wallpaper.

**The cut.** `scripts/assemble.mjs` writes one ffmpeg filter graph: segments in order, `xfade
dissolve` of 12 frames between beats, overlay PNGs with their in and out times, the music bed under
it all with a 2 s fade at each end, and one final scale-safe encode to `unluminous-demo-4k.mp4`.

**What is checked before it ships.** The output is 3840x2160, its duration is within a few seconds of
this sheet, every beat's segment is non-black and has motion in it (mean frame difference over a
floor), and the file plays from the site's own `<video>` element on prod.

## 6. Where it goes

Three sites, all of which already have a section built around it (`scripts/publish.mjs` writes to all
three from one encode, because three sites serving three encodes of one take is a thing that drifts):

| Site | Clip | Poster |
|---|---|---|
| jasonmcaffee.com | `public/videos/unluminous.mp4` | `public/images/video/unluminous.webp` |
| blackrainbowlabs.com | `public/videos/unluminous.mp4` | `public/images/unluminous-poster.webp` |
| unluminous.com | `public/videos/unluminous.mp4` | `public/images/unluminous-poster.webp` |

On jasonmcaffee.com it is an **Unluminous** section between Apps and Articles: a short description of
what Unluminous is, the video as the section's centrepiece with a poster frame, and a link to the
GitHub release. The section follows the site's existing reveal-on-scroll pattern and its `VideoPlayer`
component, so it behaves like everything else on the page rather than like an embed.

unluminous.com's section 05 holds `DEMO.clip = null` until there is a take that names the product
correctly, and prints `DEMO.pending` instead. Publishing sets that object.

What the site gets is a **1440p** encode rather than the 4K master: a four minute 4K H.264 is a few
hundred megabytes and the other videos on that page are 16-81 MB, so a 4K one would be the slowest
thing there by an order of magnitude. x264 rather than NVENC for this one encode, because it is judged
on bytes for a given look and x264 is meaningfully smaller at the same visual quality.

---

## 7. What was actually shot, and what it cost

Sixteen beats, `3840x2160` at 60 fps, **4:10** cut. Fourteen landed on the first take. The two that
did not were both defects in Unluminous rather than mistakes in the pacing, and both are worth writing
down because neither is visible from the command line's own answers.

### `debug breakpoint add` while the editor is scrolled produces a breakpoint that never binds

The debugger beat came back with the program run to completion and the debug tile empty, twice.
Bisected rather than guessed:

| Sequence | Result |
|---|---|
| `clear` → `add src/report.rs 50` → `start` | **paused at report.rs:50** |
| `clear` → wait 4 s → `add` → `start` | **paused at report.rs:50** |
| `clear` → `editor scroll --line 30` → `add` → `start` | ran to completion |
| `clear` → `add` → `editor scroll --line 30` → `start` | ran to completion |

So it is the scroll, on either side of the add, and it is not the stored breakpoint: the offset
written to `.unluminous/breakpoints.conf` is 2037, which is line 50 in that file, byte for byte. A
breakpoint in `main.rs` set the same way in the same session binds and pauses, and during the
session `debug breakpoint list` shows the one that bound as `src\main.rs` and the one that did not as
`src/report.rs` — the separator is the adapter's own answer coming back, so what is wrong is what is
*sent* rather than what is kept.

**A second, separate cause with the identical symptom:** restoring `.unluminous/breakpoints.conf` from git
underneath a running window. A breakpoint is a byte offset in a file the window has open, and putting
that file back under it leaves the editor holding a breakpoint the adapter will not bind. The shoot's
`restoreProject()` now restores `src` and `web` only, and the demo project stopped tracking `.unluminous/`.

### The Agent-Chat pane stops drawing, and the command line still says it is showing

`plugins pane agent-chat/chat --show` answers *"agent-chat/chat is showing on the right"* and paints
nothing at all — no ground, no divider, no composer. `plugins view agent-chat` answers the whole
conversation quite happily while this is true, so nothing an agent can ask reports it.

It is reproducible: hiding the pane once through `plugins pane … --hide` is enough, and so is a run
of ordinary panel, tab and settings commands. It draws every time on a window that has just started.
The beat is therefore shot on a fresh window with a five-command setup and no baseline.

**And a plugin pane's width cannot be set from the command line at all.** `panel size` knows only the
four built-in panels, and `settings set panes.agent-chat/chat.width` is refused as an unknown
setting — while `panes.agent-chat/chat.width` is a line in `settings.conf`. The pane was widened to
1150 by editing that file between restarts.

### Smaller things worth keeping

- **`editor insert` does not auto-indent**, so typed code carries its own leading spaces.
- **`editor rename --apply` writes files that are not open** straight to disk, so a beat that renames
  has to put the project back afterwards.
- **`editor complete --choose` takes the name**, not the row number.
- **Two Unluminouss running makes a bare `unluminous-cli` ask which one is meant**, which is right and is also
  not what a demo should show; beat 12 defines a one-line shim off camera so the commands on screen
  are the ones somebody with one window would really type.

### No music, on the first cut

`scripts/music.mjs` generates a bed on this machine with ACE-Step, seeded so it is reproducible. It
was not used: the render reported 1.4 GB free against the 4.8 GB it needed, and that was read at the
time as the machine's LLM being in the way. It was not — see section 8.

---

## 8. The task-1811 pass

Everything above was shot under the product's previous name, and the name is in the pixels: the title
card, the closing card, the window's own menu bar in every frame, and the transcript in the on-screen
terminal. No edit reaches that, so the cut is re-shot rather than re-trimmed. Three things changed
besides the name.

### The type was too small, and the window was most of the reason

Covered in section 2. Fonts up 20%, window down to 3296 x 1840. Both, because either alone leaves it
readable-if-you-lean-in rather than readable.

### The hover came out

Covered in beat 8. `debug hover` answers on the command line and paints nothing.

### There is music now, and the reason there was not is not the one written down

ACE-Step loads its 12.8 GB DiT **and** a 4B chain-of-thought LM, and that LM is a vLLM pool sized as a
*ratio of the whole card* rather than as a fixed number of gigabytes. So freeing VRAM does not help:
the pool simply grows to match, and the render is left with the same two or three gigabytes it had
before. Measured on this pass — with the machine's LLM stopped and its GPU completely clear, the
pre-flight still failed at 2.93 GB free against 4.92 GB needed.

The bed asks for no chain-of-thought at all (`thinking:false`, every `useCot*:false`), so the LM is
9.9 GB held for something the request never uses. Skipping it renders **265 seconds of audio in 29
seconds**, on a card that also had the desktop on it.

Two smaller things came out of the same hour:

- **`music.mjs` was polling an endpoint that cannot be polled that way.** It called ACE-Step's own API
  directly and asked `GET /query_result`, which that server answers **405 Method Not Allowed**. Every
  poll therefore read `unknown`, the script gave up after twenty minutes, and the job it was waiting
  for had failed in the first second. It now goes through the ai-service backend — `POST
  /music/generate`, `GET /music/jobs/:id` — which is the path the Music Creator page uses, so the
  track lands in the music library and the status is one the caller can actually read.
- **One render is not enough.** The first seed had an eight-second hole in the middle of it: RMS fell
  to −60 dB at 119 seconds, which under a video reads as the audio having broken rather than as an
  arrangement choice. `scripts/music-candidates.mjs` renders a set of seeds and reports the quiet
  stretches in each; seed **8807** holds an even level for the whole 4:25 and is what is pinned.

### Section 7's defect list, re-checked on 0.37.1

| | |
|---|---|
| A breakpoint added while the editor is scrolled never binds | **fixed** — it binds and pauses, with the scroll confirmed at 1927 px before the add |
| A plugin pane's width cannot be set from the command line | **fixed** — `settings set panes.agent-chat/chat.width` takes, and `panel size agent-chat/chat --width` now knows plugin panes too |
| `editor insert` does not auto-indent | **still true**, and the harness still types its own leading spaces |
| `editor rename --apply` writes files that are not open | **still true** — `src/main.rs` went to disk unopened; the harness still puts the project back |
| `editor complete --choose` takes the name | as documented, and the harness already does |
| The Agent-Chat pane stops drawing | checked on camera during the shoot; it is not a thing the command line can report on, which was the original finding |

**And a new one, which cost the first hour of this pass.** The debugger refused *every* breakpoint,
scrolled or not, and said so only in `debug output`:

```
Breakpoint at ...	ask-1793-unluminous-videoleet\src
eport.rs:50 could not be resolved,
but a valid location was found at ...	ask-1793-quill-videoleet\src
eport.rs:50
```

`fleet.exe` and `fleet.pdb` were built in September under the old folder name, and a PDB records the
absolute source paths it was compiled from. Renaming the folder left the debug information pointing at
a directory that no longer exists, so LLDB could not match a single breakpoint. `cargo build` does not
notice — the sources are unchanged, so it answers "Finished in 0.01s" and leaves the stale PDB in
place. Touching the sources forces the rebuild that fixes it.

It is worth writing down because it is invisible from every surface an agent would check: the
breakpoint is listed, the file is right, the line is right, the stored offset is right, and the only
place the truth appears is the debug console's own text. **Any beat that renames or moves this project
has to rebuild it before the debugger beat is shot.**
