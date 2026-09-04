# What Unluminous looks like

Twenty-two captures of Unluminous running on Windows, taken from the real window rather than rendered
offscreen, and cropped with a margin of desktop left round the edge. The margin is there on purpose:
Unluminous's background is translucent, and a picture cropped tight to the window cannot show that the
green in it is the desktop rather than a shade someone chose.

They were taken from **`unluminous 0.1.0`**, after `task-1658`, on a 3840 by 2160 screen with the
window at 1800 by 1160. Nothing in them is a mock up; every one is a photograph of the program
working.

`README.md` says what Unluminous is and how to run it. This file is the same ground covered in
pictures.

## What these pictures are, and what they are behind

**They are 0.1.0.** The product has had thirty-four minor versions since, and this page is honest
about the gap rather than quiet about it: what is in a picture is what 0.1.0 looked like, and the
list below is what has changed. `task-1804` §6 asked for this pass, and the part of it that is words
is done — what is not done is a re-shoot, and `documentation/README.md` beside this file says exactly
what one needs and why it was not done in the session that wrote this.

Two things changed in every picture at once:

- **Themes**, added by `task-1776`. A theme says what every colour in Unluminous's own palette means,
  and one that names the nine token colours also colours code. `Themes Bundle 1` ships five. Every
  colour in every picture below is `unluminous/dark`, which is still the default and still what a
  fresh window comes up in — but it is now one of six rather than the only one.
- **The icon set.** The rail and the explorer's folder arrow are drawn in `classic` in these
  captures, and `material` is what a window comes up in now.

And these came after, so they are in none of them:

| | Added by |
|---|---|
| `Go to File` and `Find in Files` | `task-1659` |
| **Find and Replace in the open file**, on `Ctrl/Cmd+F` and `Ctrl/Cmd+H`, and the `Find` menu they live on | `task-1804` |
| Pictures drawn in the Markdown preview rather than shown as their alt text | `task-1659` |
| Tables drawn as tables, fenced code coloured by its language, a preview that can be selected and copied | `task-1685` |
| Folding, breakpoints and the gutter's marks | `task-1686`, `task-1687` |
| The debugger: stepping, stack frames, value tooltips and inline values | `task-1688` |
| Split panes, and the explorer following the tab | `task-1664` |
| Go to Definition, Find References and Rename Symbol | `task-1675` |
| Auto-complete, and completing an import | `task-1678`, `task-1680` |
| Panels docked to any edge, and dragged between them | `task-1697` |
| A **browser** tab, on WebView2 and WKWebView | `task-1756` |
| The **Agent-Tasks** board, drawn with depth | `task-1765` |
| The **Agent-Chat** pane | `task-1767` |
| The **Database** plugin | `task-1777` |

The Database plugin has a gallery of its own, and it is current:
[`documentation/database.md`](database.md), nine captures taken the same way after `task-1777`. The
browser tab, the chat pane and the board have none, which is the largest hole in this page.

---

## The window

A title bar Unluminous draws itself, holding the menus at the left, the project's name after them, and the
text options and the three Markdown view modes at the right beside the window buttons. Down the far
left a thin rail with a button for each pane. Then the file explorer with its filter box, a tab for
each open file, the line numbers, the editing area, and a status bar naming the file, its kind, the
caret's position, the branch and the font.

The desktop is visible through the rail, the explorer, the editing area and the status bar, and every
piece of text on top of it is solid. That is the whole character of the product, and it is the first
thing to look at in any of these pictures.

![Unluminous open on a Markdown file, the desktop showing through the window](images/01-unluminous-window.jpg)

## The desktop shows through, and how far is a setting

`Settings -> Appearance -> Background` is one slider from 5 per cent to 100. At the bottom of its
range the window is nearly all desktop and the text still reads; at the top it is a solid dark
editor and only the margin round the window gives the wallpaper away. Text is painted at full
opacity at every setting, so turning the background down never makes the document harder to read.

It works on both platforms, but Windows takes three separate things to get there that macOS does not
need. All three are in `services/windows_transparency.rs`, and section 9.2 of
`tasks/unluminous-technical-design-document.md` records how each was measured.

![The same window at 15 per cent background opacity](images/10-opacity-low.jpg)

![The same window at 100 per cent, where only the margin shows the desktop](images/11-opacity-full.jpg)

## The rail, and the window's own edges

Down the far left is a button for each pane: the explorer, git, and the terminal at the bottom left.
It is 36 points wide, narrower than the reference editor's, because it holds three buttons rather than a dozen.
A button whose pane is open is drawn as the same filled pill every list in Unluminous uses for its chosen
row, so the rail says at a glance what is showing. Resting on one names it.

The rail is the only way a pane is put away and brought back. There used to be a small button
floating over the editing area when the explorer was hidden, and it is gone: the rail is in the same
place whether a pane is showing or not, which is the point of having one.

Below, the explorer has been put away and the terminal brought up, both from the rail.

![The rail with the explorer hidden and the terminal open](images/21-activity-bar.jpg)

The window is dragged by its title bar and resized by any of its four edges or four corners. Unluminous
draws those grips itself, invisibly, because the window is created with no operating system frame —
rounded corners and a translucent background need the decorations turned off — and a window with no
frame has no resize grip of its own.

## The text options are behind one button, in the title bar

Bold, italic, underline and strikethrough, five colours, four alignments and three line spacings, all
behind the `F` at the right of the title bar, next to the window buttons. The panel is four named
rows with a rule between what applies to the selected text and what applies to the paragraph it is
in. It stays open until the pointer goes elsewhere, so a colour and an alignment are two clicks
rather than two visits.

They used to sit in a strip of their own, forty four points tall, between the title bar and the tabs.
The strip was drawn for a `.md` file and not for a `.rs` one, which is the right rule and the wrong
place for it: every time the tab changed, the tabs, the explorer and the whole editing area moved up
or down by forty four points. In the title bar the room is already there whether the tools are in it
or not.

![The text options panel open under its F button](images/19-text-options.jpg)

Below, the third line has been made bold and red from that panel, the heading under it centred and
the line after it italic. The dot on the tab, against the file in the explorer and in the status bar
is how Unluminous says there are changes that have not been saved.

![A document with bold, colour, italic and a centred heading applied from the panel](images/16-formatting.jpg)

## Nothing is shown that does not apply to the file

The `F` button is drawn for prose — a `.md` file, a `.txt` file, a document that has not been saved
anywhere yet. Unluminous saves plain text and carries no formatting to disk, so for a `.rs` or a `.json`
file every one of those controls is a decoration that lasts until the file is reopened, and the three
view modes offer the Markdown parser's reading of a file that was never Markdown. So a source file
gets neither, and the right hand end of the title bar is simply empty — which is the next picture but
one.

The two questions are asked separately. A `.txt` file is prose, so it keeps the `F` button; it is not
Markdown, so it loses the view modes.

## Markdown, three ways

The three buttons beside the `F` switch between the raw source, the source and the preview side by
side, and the preview on its own. The parser is ours and it draws nothing: it reads the source and
produces the same rope, character spans and paragraph settings any other document holds, so the
preview is laid out and painted by the ordinary engine. Nothing in the window knows how to render
Markdown.

The preview is read only, because what it shows is worked out from the source beside it.

![The Markdown source and its preview side by side](images/02-markdown-side-by-side.jpg)

![The preview on its own](images/03-markdown-preview.jpg)

## Mermaid diagrams are drawn, not shown as code

A `.mmd` file gets the same three view modes a Markdown file has, named after what it actually is:
`Raw Mermaid`, `Side by side` and `Mermaid diagram`. There is no `F` beside them, because a diagram
is not prose and nothing behind that button means anything in one.

The picture is Unluminous's own drawing. Nothing runs `mermaid.js` — `tasks/unluminous-mermaid-plugin-tdd.md`
weighs the three ways of doing that and says why none of them belongs in a text editor — so a
diagram is rectangles, circles, polygons, lines and text worked out by `unluminous_core::mermaid` and
painted by the same painter that draws everything else. Which is why the leaves are still visible
through it: a diagram is drawn into the window rather than pasted over it.

Twenty diagram types are drawn. Ten more are **named** rather than drawn, in a panel saying which
type it is above the source, and a diagram that will not parse says which line went wrong and shows
that line — both of which are more use than an empty pane.

![A flowchart drawn from a .mmd file, with the desktop showing through](images/25-mermaid-diagram.jpg)

A fence whose language is `mermaid`, inside a Markdown document, is drawn in that document's
preview, in the room its paragraph reserved. A fence in any other language is still shown as code.

![A Markdown document whose preview draws the diagrams in it](images/26-mermaid-in-markdown.jpg)

## Writing code in it

Line numbers down the left, a tab for each open file, syntax colouring from a plugin — and no text
options in the title bar, because there is nothing in them that means anything for a Rust file. Unluminous
wraps, so a paragraph that runs over several rows on screen carries one number against its first row
and nothing against its continuations, which is what a line number means everywhere else.

A single click in the explorer opens a file in the tab a single click reuses, drawn faintly to say
so; a double click opens it in a tab of its own. The tab that is showing carries an accent line
along its bottom edge.

The colouring comes from the Rust plugin. A colour scheme colours the tokens and not the editing
area, so a coloured file still lets the desktop through — which is why the leaves are still visible
behind the code.

![Four files open in tabs, with line numbers, syntax colouring and no text options](images/04-code.jpg)

## The font is one setting for the whole window

`Settings -> Appearance -> Font` sets the family and the size the editor shows text in, the way
The reference editor has one editor font. A change reaches every file that is open, not only the one showing,
and the preview with them. Setting it is not an edit: it pushes nothing onto the undo history and
does not mark any file as changed, because what Unluminous saves is plain text and carries no formatting.

The size is on the keyboard as well — command or control with plus and minus, and `Reset Font Size`
to put it back — and on a trackpad pinch, or the wheel with the same modifier held, over the editing
area. All of them change that one setting, so whichever is used the size is still there next time
Unluminous starts. `+` and `=` are one key on nearly every layout, so either does it, with or without
shift, and so does the keypad's `+`.

![The settings window on the Appearance page](images/08-settings-appearance.jpg)

## A picture opens in a tab

`.png`, `.jpg`, `.gif`, `.bmp`, `.ico`, `.webp` and `.tiff` open in a tab that shows them. A picture
is scaled to fit the editing area to begin with, because a photograph four thousand pixels across
shown from its top left corner would be a viewer you have to zoom out of before it shows anything;
one smaller than the area is left at its own size rather than blown up. The status bar says how big
it is and how far it is zoomed.

Command or control with plus and minus zooms it, and so do the wheel with that modifier held and a
pinch on the trackpad — the same keys and the same gestures that size the editor's text, aimed at
whatever the tab is holding. Dragging moves it, and a double click puts it back to filling the area.
A picture cannot be edited, so `Save` says so and writes nothing.

![A photograph open in a tab, scaled to fit the editing area](images/22-picture.jpg)

## The file explorer

Folders expand in place rather than replacing the list, so where a file sits stays visible. Each row
carries an icon for its kind, and files git knows something about are tinted by what it thinks of
them. The footer counts the files and how many of them can be opened. The filter box above the list
matches file names anywhere in the tree, not only the rows that happen to be showing.

The divider between the explorer and the editing area is dragged to resize it, and a double click on
the divider puts it back to its usual width. The split between the Markdown source and its preview
and the height of the terminal work the same way, through the same code, and where each was left is
written to the settings file so it is the same next time Unluminous starts.

![The explorer widened by dragging its edge, with its folders expanded](images/17-explorer.jpg)

Right clicking a row opens a menu aimed at that row: a `New` submenu, cut, copy, copy path and
paste, rename, show the file in Explorer, reload it from disk, and a `Git` section that acts on that
file alone. Cut and paste go through Unluminous's own clipboard rather than the operating system's, and
pasting onto a name that is already taken adds a number rather than overwriting what is there.

![The right click menu on a file in the explorer](images/05-explorer-menu.jpg)

## The menus, and a project is a window

`Unluminous`, `File`, `Edit`, `Find`, `View`, `Run` and `Git`, and one more for each plugin that
contributes one — `Agent-Chat`, `Agent-Tasks` and `Database` all do. The pictures below are from
0.1.0 and show five of them: `Run` arrived with the run configurations, and `Find` with
`task-1804`, which took Find, Replace, Find in Files and the three symbol entries out of an Edit menu
that had run off the bottom of a short window. On macOS they are in the bar along the top of the screen,
where macOS puts menus; on Windows they are drawn at the left of Unluminous's own title bar and the three
window buttons move to the right hand end, where Windows puts them. Both bars are built from one
list, so they hold the same entries with the same shortcuts, and adding an entry adds it to both.

The `File` menu opens another window, opens a file or a folder, and lists the folders that have been
open before. **Opening a folder opens it in a window of its own**, the way `Recent Projects` does and
the way the reference editor works, so the project you were in stays where it was. Each window is its own
process, so several Unluminouss can run at once on different projects and share nothing but the settings
file.

What each project had open — its tabs, which of them was showing, which folders in the explorer were
opened out, whether the terminal was up and how many tabs it had — is written into a `.unluminous` folder
beside the project, and put back when the project is opened again. It sits with the code rather than
in the settings folder, so copying the project copies its state.

![The File menu, with the recent projects listed in it](images/18-file-menu.jpg)

The `View` menu holds the three Markdown modes, the explorer, the line numbers, the editor's font
size, the file tabs and the terminal — and, since these pictures, the panes each plugin contributes,
splitting the editing area, and filling the window with one pane. The three modes are dimmed for a
file there is nothing to preview of, which is the same question the buttons in the title bar are
drawn from.

![The View menu open](images/20-view-menu.jpg)

The `Git` menu is the whole of git: commit, add, diff, compare with a revision, history, blame,
rollback, push, pull, fetch, merge, rebase, branches, tags, reset, stash, remotes and clone. It is
dimmed when the folder is not in a repository, and it grows `Continue` and `Abort` while a merge or
a rebase has stopped on a conflict.

![The Git menu open](images/06-git-menu.jpg)

## The terminal

A tile along the bottom of the window with a tab for each shell, opened from the bottom of the rail,
with control and backtick, or from the `View` menu. Each tab runs the shell in the folder the
explorer is showing, and is named after the title the program set. It handles colour including 24 bit
colour, bold, italic, underline, inverse and dim, wide characters, the alternate screen a full screen
program draws on, ten thousand lines of scrollback, selecting with the mouse, and mouse reporting for
a program that asked for it.

![Two terminal tabs, with coloured git output in the second](images/07-terminal.jpg)

## Git

Unluminous runs the `git` program rather than binding a library. What matters is that the machine's own
git is already configured — a credential helper, an ssh agent, signing, hooks, an identity for this
repository in particular — and a push from Unluminous has to be the same push you get in a terminal.
Every command runs on a thread, one at a time, so the window never stops drawing to wait for one and
two commands cannot fight over `index.lock`. When something goes wrong, what the status bar shows is
git's own message, because a rejected push and a merge conflict explain themselves better than
anything Unluminous could say about them.

`Commit...` opens a panel with a changes tree, a tick box a file, the repository's row carrying its
branch, an `Unversioned Files` group, `Amend`, the counts, the message box with the last twenty
messages behind a button, and `COMMIT` and `COMMIT AND PUSH...`. Ticking a file stages it at once,
so Unluminous's idea of what is staged and git's cannot disagree while the panel is open. The rail's git
button opens the same panel and shuts it again.

![The commit panel, with a changed file and two untracked ones](images/15-git-commit.jpg)

`Show History` lists the commits, each with its hash, its message, its author and its date, and marks
the commit `HEAD` is sitting on. With a file open it is that file's history; with none, the
repository's.

![The history of the repository, three commits by three authors](images/13-git-history.jpg)

`Show Diff` shows git's own diff for the file against the version git has. The stripe in the gutter
beside the changed line in the editor behind it is the same information in the margin: a change bar
against each line that differs from the committed version.

![The diff for a file that has an uncommitted line](images/14-git-diff.jpg)

`Annotate with Git Blame` puts a column beside the line numbers with the date and author of the
commit each line came from, coloured by age: the oldest commit in the file green, the newest pink,
and everything between interpolated by rank. It is the fastest way to see that a file is three
authors' work and which part is new.

![A file annotated with git blame](images/12-git-blame.jpg)

## Settings

`Edit -> Settings`, `Unluminous -> Settings`, or control and comma, opens a modal laid out like
The reference editor's: the pages down the left under their headings, and the chosen page on the right. Changes
take effect as they are made, and are written to two plain text files that can be read and edited by
hand — `%APPDATA%\Unluminous` on Windows and `~/Library/Application Support/Unluminous` on macOS. What belongs
to a *project* rather than to a person is not kept there; it is in the project's own `.unluminous` folder.

`Plugins` is the marketplace and what is installed. A plugin is a folder holding a `plugin.conf` and
an icon, in the same `name = value` format the settings file uses, and it describes a language: its
extensions, its keywords, what a comment and a string look like, and a colour per kind of token.
**Nothing in one is executed**, so installing one is copying a folder. Four ship with Unluminous —
CSS, JavaScript, TypeScript and Rust — and each page says plainly what its colouring does not
handle.

![The settings window on the Plugins page](images/09-settings-plugins.jpg)

---

## How these were taken

They are captures of the screen, not renders. `crates/unluminous-app/tests/screenshots.rs` builds the
same window offscreen and writes a PNG for each of its tests, and those images are the right ones for
checking that a control moved or a colour changed; they cannot show that the operating system
honoured the window's transparency, because there is no desktop behind them.

So each of these was taken by starting the real `unluminous.exe`, putting the window at a fixed
rectangle, driving it with real mouse clicks and key presses, and copying the screen. The crop is the
window's rectangle grown by 48 pixels on every side, which is the margin of desktop you can see.

**The scripts that did it are not in this repository**, and that is the reason the gallery went stale
rather than an excuse for it. They live in `_agent_output/task-1658-screenshots/`, which is
gitignored — `build-fixture.ps1`, `unluminous-capture.ps1` and the seven `stage-*.ps1` files, plus
`_agent_output/task-1660-mermaid/capture-diagram.ps1` for the two diagram pictures. They exist on one
machine, they are not in any checkout, and a page whose recipe only one computer has is a page nobody
else can bring up to date.

They also predate `task-1762`, which is the other half of why they cannot simply be committed as they
stand: they press keys with `keybd_event` directly, and `CLAUDE.md` now says that
**`tools/windows-input.ps1` is the one way a script sends keyboard or mouse input**, because a run
that stops between a key going down and coming up leaves that key held for the rest of the session
with nothing on the screen to say so. Moving them into `tools/` means putting them through that file
first.

[`documentation/README.md`](README.md) is what a re-shoot needs, written down where the next person
will look for it.

Two things those scripts do that matter. The project in the pictures is a fixture built under the
temporary folder rather than a real one, because `sample/` lives inside Unluminous's own repository, so
opening it where it lies makes the status bar say how many files happened to be uncommitted that
day; a copy with three commits of its own says `main`, which is what a reader with a fresh checkout
sees. And the window is given a settings folder of its own, through its own `APPDATA`, so the
pictures carry a fixed font size, opacity and explorer width rather than whatever the person running
them happens to have set — and taking them leaves nothing in anybody's real settings.

They are JPEGs rather than PNGs. Most of what is in each picture is a photograph showing through a
translucent window, which PNG stores badly: the same captures are several times the size as PNG at
no visible difference in the text at full size.
