# Quill

A text editor for macOS and Windows, written in Rust. It opens any file holding text, has a file explorer
with folders that expand in place, a terminal along the bottom with tabs, and it lets the desktop show
through its background while the text stays solid.

It is also, now, an editor you can write code in: line numbers down the left, a tab for each open
file, a right click menu on the explorer, git in full, and plugins that colour JavaScript, TypeScript
and Rust.

## Installing it

```powershell
powershell -File installer\windowsuild.ps1 -Install     # Windows
```
```bash
installer/macos/build.sh --install                        # macOS
```

`installer/` builds a real installer for each platform out of the same drawn icon: on Windows a single
`QuillSetup-<version>-x64.exe` that puts Quill in the Start Menu, on the PATH and in *Open with*, and
on macOS a `Quill.app` and a disk image to drag into `/Applications`. `installer/README.md` says what
each switch does; `tasks/quill-installer-tdd.md` says why it is built the way it is.

## Running it

```
cargo run --release -- sample/welcome.md
```

The argument is a folder to show in the explorer, or a file to open, in which case the explorer shows the
folder that file is in. With no argument the explorer shows the current directory.

Switches, all of which exist so a starting state can be chosen without clicking, which is what makes it
possible to capture the window in a particular state:

| Switch | What it does |
|---|---|
| `--opacity N` | The starting background opacity, from 0.05 to 1.0. The same setting as `Settings -> Appearance -> Background`. |
| `--view raw\|side\|preview` | Which of the three ways of looking at a Markdown file it starts on. |
| `--terminal` | Open the terminal at the bottom straight away. |
| `--menu-bar native\|in-window` | Where the menus are drawn. macOS uses the bar along the top of the screen and everything else uses Quill's own title bar; naming it is how the bar inside the window can be looked at on a Mac. |
| `--print-menus` | Print the menus and their shortcuts, and stop. The macOS menu bar cannot be read by a test, so this is how what went into it can be checked. |

Several Quills can run at once, each on its own project. `File -> New Window` opens another window on the
same project, `File -> Open Folder in New Window` opens one on a folder you choose, and
`File -> Recent Projects` opens one on a folder that has been open before. Each is its own process, so they
share nothing but the settings file.

## What the window looks like

A title bar Quill draws itself, the formatting toolbar, the file explorer down the left with a filter box,
the editing area, the terminal along the bottom when it is showing, and a status bar. The palette was read
out of `design/intial-design-screenshot.png` rather than chosen by eye; run `cargo run --example sample_design`
to print the colour of each region of that image.

`documentation/overview.md` is the whole of this file in pictures: eighteen captures of the running window,
each cropped with a margin of desktop round it so that what shows through the background is visible.

`design/verification/live-window-over-desktop.png` is a capture of the running window over a real desktop.
The wallpaper is visible through the explorer, the editing area and the status bar, and every piece of text
is solid on top of it, which is what the opacity setting is for. It was taken before the font controls moved
into the settings, so its toolbar has two boxes at the left that the toolbar no longer has.
`design/verification/terminal-claude.png` and `terminal-codex.png` are captures of `claude` and `codex`
running in Quill's terminal, with `terminal-claude-resized.png` and `terminal-codex-resized.png` showing the
same programs after the tile was made shorter and the explorer wider.

## The menus

`Quill`, `File`, `Edit`, `View` and `Git`, in that order, with `Quill` first. On macOS they are in the bar along the
top of the screen, where macOS puts menus. On Windows they are drawn at the left of Quill's own title bar,
and the three window buttons move to the right hand end, where Windows puts them.

| Menu | What is in it |
|---|---|
| `Quill` | About Quill, Settings, Quit. |
| `File` | New Window, Open File, Open Folder, Open Folder in New Window, Recent Projects, Save, Save As, Close Window. |
| `Edit` | Undo, Redo, Cut, Copy, Paste, Select All, Settings. |
| `View` | The three view modes, show or hide the explorer, show or hide the line numbers, close a tab and move between tabs, show or hide the terminal, a new terminal tab. |
| `Git` | Commit, Add, Show Diff, Compare with Revision, Show History, Show Current Revision, Annotate with Git Blame, Rollback, Push, Pull, Fetch, Merge, Rebase, Branches, New Branch, New Tag, Reset HEAD, Stash, Unstash, Manage Remotes, Clone. Dimmed when the folder is not in a repository, and it grows `Continue` and `Abort` while a merge or a rebase has stopped on a conflict. |

Both bars are built from one list, so they hold the same entries with the same shortcuts. Run
`quill --print-menus` to see it.

## Settings

`Edit -> Settings`, `Quill -> Settings`, or command and comma, opens a modal laid out like IntelliJ's: the
pages down the left under their headings, and the chosen page on the right.

- `Appearance & Behavior -> Appearance -> Font` sets the family and the size the editor shows the document
  in. It applies to the whole document and leaves bold, italic and colour as they were. It is not an edit:
  it pushes nothing onto the undo history and does not mark the file as having unsaved changes, because what
  Quill saves is plain text and carries no formatting.
- `Appearance & Behavior -> Appearance -> Background` sets the background opacity, which is what lets the
  desktop show through the window. It works on both platforms, though Windows takes three things to get
  there that macOS does not need, all of them in `services/windows_transparency.rs`: wgpu is told to use
  DX12, because left to choose it picked Vulkan, whose surface offers no transparent composite mode;
  the swapchain is built from a DirectComposition visual, because one built from a plain window handle
  can only be `Opaque`; and the window's redirection surface — the GDI bitmap the desktop window manager
  composites the window from — is filled with black once a frame, because winit asks the manager to
  honour its alpha but never clears it, so it holds undefined bytes that read as opaque white. Without
  the last of those the window really does fade, but towards white rather than towards the desktop.
  Section 9.2 of `tasks/quill-technical-design-document.md` records how each was measured and what was
  rejected.
- `Editor -> Editor -> Gutter` shows or hides the line numbers.
- `Plugins` is the marketplace and what is installed.
- `Tools -> Terminal` sets the size the terminal draws its grid at.

Changes take effect as they are made. The settings, the recent projects and where the dividers between the
panes were left are kept in `~/Library/Application Support/Quill` on macOS and `%APPDATA%\Quill` on Windows,
in two plain text files that can be read and edited by hand.

## Writing code in it

**Line numbers** down the left of the editing area. Quill wraps, so a paragraph that runs over several
rows on screen carries one number against its first row and nothing against its continuations, which
is what a line number means everywhere else. Right clicking the gutter puts them away or annotates
the file with git blame.

**A tab for each open file.** A single click in the explorer opens a file in the tab a single click
reuses, drawn faintly to say so; a double click opens it in a tab of its own, and so does typing into
a tab you were only glancing at. The tab that is showing carries an accent line along its bottom
edge. `Ctrl+Tab` and `Ctrl+Shift+Tab` move between them and `Ctrl+F4` closes one.

**A right click menu on the explorer**: New > File with any extension you like, Cut, Copy, Copy Path,
Paste, Rename, Show in Explorer or Reveal in Finder, Reload from Disk, and a `Git` submenu aimed at
that row. Cut and paste go through Quill's own clipboard rather than the operating system's, so a
file cut in Quill cannot be pasted in Explorer; pasting onto a name that is taken adds a number
rather than overwriting what is there.

**Git**, in the `Git` menu, in the same submenu on any explorer row, and in three places you do not
have to ask for: the branch and how far it is from its upstream in the status bar, each file in the
explorer tinted by what git thinks of it, and a change bar in the gutter against each line that
differs from the version git has.

`Commit...` opens a panel laid out like IntelliJ's: a changes tree with a tick box a file, the
repository's row carrying its branch, an `Unversioned Files` group, `Amend`, the counts, the message
box with the last twenty messages behind a button, and `COMMIT` and `COMMIT AND PUSH...`. **Ticking a
file stages it at once**, so Quill's idea of what is staged and git's cannot disagree while the panel
is open.

Quill runs the `git` program rather than a library, so a push from Quill is the same push you get in
your terminal — the same credential helper, the same ssh agent, the same hooks, the same signing.
When something goes wrong it shows **git's own message**, because a rejected push and a merge
conflict explain themselves better than anything Quill could say about them. Every command runs on a
thread, so the window never stops drawing to wait for one. `Rollback`, a hard `Reset HEAD` and
dropping a stash each ask first, because none of them can be undone. Pushing with force always uses
`--force-with-lease`.

A merge or a rebase that stops on a conflict is not hidden: the status bar says so, the conflicted
files are marked, the Git menu grows `Continue` and `Abort`, and the file opens with its markers in
it — which is a file holding text, and therefore something Quill already edits.

**Plugins** colour a file by what its text is. Three ship with Quill — JavaScript, TypeScript and
Rust — and each gives its files an icon, a set of words to colour, and the Dracula colour scheme. A
plugin is a folder holding a `plugin.conf` and an icon, in the same `name = value` format the
settings file uses; **nothing in one is executed**, so installing one is copying a folder.
`Settings -> Plugins` lists them, and `Install` writes a plugin's folder out where it can be edited
by hand.

A colour scheme colours the tokens and not the editing area, so a coloured file still lets the
desktop through.

## What it does

Editing, in the modes that show the source: select with the mouse or with shift and an arrow key, cut, copy,
paste, move the caret by character, by word, to the start or end of a line, and to the start or end of the
document, undo and redo.

Character formatting: bold, italic, underline, strikethrough and colour in the toolbar, with the family and
the size in the settings.

Paragraph formatting: left, centre, right and justified alignment, and single, one and a half or double line
spacing.

Files: any file holding text opens. A `.md` file is Markdown, which means the preview shows it rendered;
everything else opens as plain text, whether Quill knows the file type or not, so a `.rs` or a `.js` file
opens as what it is. A file that is not text, such as an image or an archive, is listed in the explorer,
dimmed, and says why it cannot be opened when the pointer rests on it. So is a file larger than 16 MB.

Panes: the explorer's width, the split between the Markdown source and its preview, and the terminal's
height are all set by dragging the divider, and a double click puts one back to its usual size. Where they
were left is remembered.

The terminal: a tile along the bottom of the window with tabs, opened with control and backtick or from the
`View` menu. Each tab runs the shell in `$SHELL` in the folder the explorer is showing. It handles colour
including 24 bit colour, bold, italic, underline, strikethrough, inverse and dim, wide characters, the
alternate screen a full screen program draws on, ten thousand lines of scrollback, selecting with the mouse,
copying with command and C, and mouse reporting for a program that asked for it. A tab is named after the
title the program set, so a tab running `claude` says so. `tasks/quill-terminal-tdd.md` sets out how it
works and what it does not do.

Keyboard: command plus B, I or U for bold, italic and underline. Command plus shift plus X for
strikethrough. Command plus L, E, R or J for the four alignments. Everything else is on a menu, and the menu
shows its shortcut. On Windows the control key takes the place of the command key.

## How it is put together

Four crates.

`crates/quill-core` is the editor. It holds the text buffer, the formatting, the caret, layout, undo and the
Markdown parser, and it has no user interface dependencies at all, so its tests run with no window, no
graphics card and no fonts. Its only dependency is `unicode-segmentation`.

The Markdown parser is worth a note. It does not draw anything. It reads the source and produces the same
three things a document holds, a rope of text with character spans over it and one paragraph setting per
line, so the preview is drawn by the ordinary layout engine and the ordinary painter. Nothing in the window
knows how to render Markdown.

`crates/quill-terminal` is the terminal, and it has no user interface dependencies either. The escape
sequence emulation and the pseudoterminal come from `alacritty_terminal`; the colour palette, the key
encoding, the mouse reports, the screen the painter reads and the tabs are ours.
`tasks/quill-terminal-tdd.md` records why that line was drawn there and what else was considered.

`crates/quill-git` is git. It has no user interface dependencies either, and it runs the `git`
program rather than binding a library: what matters is that the machine's own git is already
configured — a credential helper, an ssh agent, signing, hooks — and a push from Quill has to be the
same push you get in a terminal. It reads the formats git provides for being read rather than the
ones meant for a person, and every call hands back git's own output whether it worked or not. Its
tests build real repositories in a temporary folder and ask **git** what happened afterwards.

`crates/quill-app` is the window. It uses `eframe` and `egui` for the window, the input events, the graphics
device and the ordinary controls, `fontdb` to find installed fonts, `ab_glyph` to read and rasterise them,
`rfd` for the operating system's file pickers, `muda` for the macOS menu bar and `arboard` for the clipboard
behind the Edit menu. It is laid out in four folders: `app` for the window's state and the actions the menus
ask for, `components` for drawing, `services` for everything that is not drawing, and `theme` for the
palette and the icons. `CLAUDE.md` records the conventions a change should follow.

The text buffer, the line breaking, the alignment, the hit testing and the glyph atlas are written here
rather than taken from a library. `tasks/quill-technical-design-document.md` records why, which other
options were considered, and what was read while writing it.

## Tests

```
cargo test
```

Four layers, 347 tests.

`quill-core` has 124 unit tests, including 24 for the Markdown parser and a randomised comparison of the
rope against a plain `String` over 1500 edits with the tree invariants checked after every one. Layout tests
measure through a fixed width stub, so their expected numbers are arithmetic a reader can check and are the
same on every machine.

`quill-terminal` has 70 unit tests: every key in the encoding table, the sixteen named colours and the
colour cube, what the screen holds after a run of escape sequences, the alternate screen, scrollback,
resizing, the mouse reports and the tabs. Two of them start a real shell and wait for its output, which is
what proves the pseudoterminal, the reader thread and the writing work together.

`quill-app` has 81 unit tests covering the file explorer, its filter, what counts as a text file, the
settings file, the menus and their shortcuts, real font measurement and glyph packing.

`crates/quill-app/tests/screenshots.rs` has 72 tests that build the whole application, feed it real events,
render it through `wgpu` and write a PNG for each one to `crates/quill-app/tests/snapshots`. Those images
are meant to be looked at: they are how a person or an agent confirms that bold text is bolder, that the
settings window is laid out like the design, and that the terminal's colours are right. Once accepted they
are also the comparison baseline, so a later change that alters the rendering fails a test.

Each platform has its own accepted set, because the window is deliberately not the same on both: macOS has
the menus in the bar along the top of the screen and the window buttons at the left, Windows draws both in
Quill's own title bar, and the text is Arial rather than Helvetica because Helvetica is not installed there.
macOS reads `tests/snapshots` and Windows `tests/snapshots/windows`. Run against the macOS images, 32 of
the 50 differed on Windows for reasons that were the program working exactly as it should.

To accept new images after a deliberate change:

```
UPDATE_SNAPSHOTS=1 cargo test
```

A run that differs writes `{name}.new.png` and `{name}.diff.png` next to the accepted image.

The fourth layer is the real application, because the first three render offscreen and cannot show that the
operating system honoured the window's transparency or drew the menu bar. `cargo run --release` for the
window, and for the terminal:

```
cargo run --example terminal_capture -- --wait 10 --send "\r" --wait 10 claude
cargo run --example terminal_capture -- --wait 12 --send "\r" --wait 12 codex
```

That builds the real window offscreen, runs the program in the terminal, answers it, and writes a picture to
`design/verification`, along with a second one after the tile has been made shorter, which is where a
program that was not told its new size draws in the wrong place. The images are not compared against a
baseline, because both programs draw something different every time they run; they exist to be looked at.

## Not included

Right to left and complex writing systems. Version one places one grapheme cluster after another from
left to right, which is correct for Latin, Greek and Cyrillic and wrong for Arabic and Hindi. The
`FontMetrics` boundary is where a shaping step would go.

Search and replace, and several carets at once.

Code folding. The 12 point gap beside the line numbers is where its arrows would go, and nothing else
about the gutter would have to move; what it needs is a notion of a block, which needs a real parser.

A three way merge editor, a language server, and a marketplace that fetches a plugin over the
network. Each is named with its reason in `tasks/quill-ide-tdd.md`.

In the syntax colouring: a regular expression literal, which cannot be told from division without
parsing; nested block comments in Rust; interpolation inside a template literal; and JSX. Each
plugin says so on its own page in `Settings -> Plugins`.

In the Markdown preview: tables, footnotes, images shown as pictures rather than as their text, reference
style links, nested block quotes and HTML. Tables need layout Quill does not have; the rest are rare in
prose.

In the terminal: images, the Kitty keyboard protocol, a blinking cursor, searching the scrollback, and
choosing the shell in the settings. `tasks/quill-terminal-tdd.md` lists them with the reasons.
