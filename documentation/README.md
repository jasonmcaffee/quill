# Taking the pictures again

`documentation/overview.md` is twenty-four captures of Unluminate 0.1.0 and
`documentation/database.md` is nine of the Database plugin after `task-1777`. This file is what
taking either of them again needs, written down here because the last time it was written down it was
in a task's own scratch folder and went with it.

`task-1804` §6 called the overview gallery stale — *"a gallery that shows the wrong icon set and the
wrong colours is worse than none on a download page"* — and the pass it asked for is done in words:
`overview.md` now says which version each picture is, what changed after it, and what has no picture
at all. **The pictures themselves were not re-taken, and this is why.**

## What a capture actually is

Not a render. `crates/unluminate-app/tests/screenshots.rs` builds the same window offscreen and writes
a PNG for each of its tests, and those images are the right ones for checking that a control moved or
a colour changed. They cannot show what this gallery is *for*: Unluminate's background is translucent,
and a picture with no desktop behind it cannot show that the colour in the editing area is the
wallpaper rather than a shade somebody chose.

So a capture is a photograph of the screen, with the window sat over a real desktop and the crop taken
48 pixels wider than the window on every side.

## What that needs, and none of it is optional

- **A 3840 by 2160 screen**, with the window at 1800 by 1160. Every existing picture is that size, and
  a gallery half of which is a quarter of the resolution of the other half reads as a mistake.
- **A clear desktop with a picture on it.** Every other window out of the way first — the two diagram
  captures had a step for exactly this, because what shows through has to be the wallpaper rather than
  whatever happened to be open behind it.
- **A person looking at the result.** This is the same rule the screenshot tests keep — *"look at the
  images… nothing should be accepted without opening it"* — and it applies more here, because there
  is no baseline to compare against and the only thing that says a capture is good is somebody
  deciding it is.

The session that wrote this had a 1024 by 768 remote desktop with browsers and terminals on it. Taking
twenty-four pictures there would have replaced a stale gallery with a worse one, so it was left, said
plainly, and handed on.

## Before re-shooting, move the scripts

They are in `_agent_output/task-1658-screenshots/`, which is gitignored, so they are on one machine
and in no checkout. That is the reason the gallery went stale, and re-shooting without fixing it just
resets the clock.

They cannot be committed as they stand. They press keys with `keybd_event` directly, and `CLAUDE.md`
now says **`tools/windows-input.ps1` is the one way a script sends keyboard or mouse input** — because
a run that stops between a key going down and its coming up leaves that key held for the rest of the
session, with nothing on the screen to say so and the physical keyboard unable to clear it. That rule
was written after a run left the left Windows key down, on which every letter becomes a shortcut.

So the order is: put them through `tools/windows-input.ps1`, move them to `tools/documentation/`,
then take the pictures.

## What the scripts do that is worth keeping

Two things, and both are the difference between a picture of the product and a picture of one
person's machine:

- **The project is a fixture built under the temporary folder**, not `sample/`. `sample/` lives inside
  Unluminate's own repository, so opening it where it lies makes the status bar say how many files
  happened to be uncommitted that day. A copy with three commits of its own says `main`, which is what
  a reader with a fresh checkout sees.
- **The window is given a settings folder of its own**, through its own `APPDATA`. Without it the
  pictures carry whatever font size, opacity and explorer width the person running them has set — and
  taking them would leave the fixture's project state in their real settings.

## And what has no picture at all

The browser tab (`task-1756`), the Agent-Chat pane (`task-1767`) and the Agent-Tasks board
(`task-1765`) are the three newest surfaces in the product and none of them is in either gallery. They
are the first three to take.
