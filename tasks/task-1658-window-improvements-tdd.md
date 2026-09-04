# task-1658: the window's own frame, the rail, the project's state, and pictures

Seven asks in one ticket, and they touch enough of the window between them to be worth writing down
together: what each one chose, what it rejected, and what the choice cost.

- [1. The window can be resized from every edge](#1-the-window-can-be-resized-from-every-edge)
- [2. The pointer is a bar over the writing](#2-the-pointer-is-a-bar-over-the-writing)
- [3. Opening a folder opens a window](#3-opening-a-folder-opens-a-window)
- [4. The rail of pane buttons](#4-the-rail-of-pane-buttons)
- [5. What a project remembers](#5-what-a-project-remembers)
- [6. The tools moved into the title bar](#6-the-tools-moved-into-the-title-bar)
- [7. A picture opens in a tab](#7-a-picture-opens-in-a-tab)
- [8. What was checked, and how](#8-what-was-checked-and-how)

---

## 1. The window can be resized from every edge

**The fault.** Unluminate's window is created with `with_decorations(false)`, because rounded corners and a
translucent background need the operating system's own frame turned off. An undecorated window has no
frame, and a window with no frame has nowhere for the window manager to offer a resize grip. The
window could be resized from the top, where the title bar's drag happened to land on something Windows
still handled, and from nowhere else — not wider, not shorter, not from a corner.

**What was chosen.** `components/resize_edges.rs`: eight invisible grips inside the window's own
rectangle, four edges [`EDGE`] = 6 points wide and four corners [`CORNER`] = 16 points square. Each
sets the pointer to the arrow for the direction it moves, and a drag on one sends
`egui::ViewportCommand::BeginResize`, which `egui-winit` turns into `Window::drag_resize_window` and
which hands the whole drag to the window manager.

Three details that are not obvious and that a later change must keep.

**They are added to the `Ui` last.** This is the same rule `components/splitter.rs` already records
about dividers, wearing a different hat: egui gives a pointer to the *last* widget added that wants
it, and the editing area, the explorer and the status bar all take drags over the whole of their
rectangles. Added anywhere earlier, the grips never see a pointer at all.

**The corners are added after the edges**, so a grab where the two overlap resizes both ways rather
than one.

**The drag is reported once, on the frame it starts.** `BeginResize` gives the pointer to the window
manager until the button is let go; sending it again on the next frame asks for a second resize inside
the first.

**Why six points and not eight.** Eight is what `splitter::GRAB` uses. Six is what these use, because
the grips sit **over** everything at the window's edge and the activity bar's buttons are inset by
exactly six — so a button and a grip never want the same point. The two numbers are tied together and
there is a test in `activity_bar` that says so.

**Rejected: a visible frame.** Drawing a one point border to grab would be an operating system window
frame drawn by hand, which is the thing turning the decorations off was for.

**Rejected: widening the title bar's drag area.** That moves the window; it does not resize it, and it
answers none of the four edges the ticket names.

---

## 2. The pointer is a bar over the writing

One line in `UnluminateApp::show_editor`: `CursorIcon::Text` while the editing area is hovered.

The ordering matters and it is already right for free. The gutter is a rectangle of its own and the
explorer's divider sets `ResizeHorizontal`, and both are drawn **after** the editing area, so where
they overlap the last one to speak wins — which is what a scan of the real window across the divider
shows: I-beam over the text, `sizewe` over the divider, arrow over the explorer.

The Markdown preview deliberately does not get it. There is nothing to type into a preview.

---

## 3. Opening a folder opens a window

`File -> Open Folder` used to replace the project in the window it was chosen from, and
`File -> Open Folder in New Window` sat under it doing what the ticket now asks `Open Folder` to do.
Two entries that differ only in which window the project lands in is one entry too many, so there is
one now, on `Cmd/Ctrl+Shift+O`, and it starts a second process — exactly as `Recent Projects` already
did, and as IntelliJ does.

`services::launcher::open_window` is unchanged; only what calls it changed. If a second process cannot
be started the folder takes this window instead, which is better than the entry doing nothing.

**`open_folder` is still there** and is still what replaces the project in this window, because
`Open File` uses it when the file chosen is outside the folder that is open.

---

## 4. The rail of pane buttons

`components/activity_bar.rs`. A 36 point rail down the far left of the body: `Project` and
`Version Control` at the top, `Terminal tile` at the bottom left, which is where the ticket's own
reference capture of IntelliJ puts it. Narrower than IntelliJ's forty, because Unluminate's holds three
buttons rather than a dozen.

**The names.** Not `Git`, because the menu bar already has a `Git` and no two controls in one window
may share a name — a test cannot tell them apart and neither can anybody reading them out. Not
`Terminal` either: `Edit -> Settings` has a page called `Terminal` and the `View` menu has an entry
called `Terminal`. `tile` is what the rest of Unluminate already calls the thing along the bottom.

**The look.** A button that is on is the pill every list in Unluminate draws for its chosen row —
`SELECTED_ROW`, corner radius `CONTROL_CORNER` — rather than a filled `ACCENT` square. Three bright
blue squares in a rail that is nearly always in that state would be the loudest thing in the window,
and a pane being open is a state rather than a press.

**What it replaced.** A small disclosure button that floated over the top left of the editing area
when the explorer was hidden, and nothing at all for the terminal. The rail is in the same place
whether a pane is showing or not, which is the point of having one, so that button is gone.

**The icons are drawn, not generated.** The ticket asks for icons made with Ideogram or Krea. They are
three strokes each in `theme::icon` instead — `folder`, `branch`, `terminal` — and the reason is
written down in the style guide and was settled in `task-1657` for the `F`: every icon in Unluminate is
tinted where it is used, and these three need three tints apiece (`TEXT_DIM` sitting there,
`TEXT_STRONG` when the pane is open, `TEXT_FAINT` dimmed outside a repository). A raster picture can
be neither tinted nor drawn at another scale without resampling it, so one generated icon among
seventeen drawings would be the one that looked wrong, and a rail of three of them would be three.
If the drawn ones are not liked, the answer is to draw them better rather than to change the medium.

---

## 5. What a project remembers

`services/project_state.rs`, and a `.unluminate` folder **inside the project**, beside `.idea` and
`.vscode` rather than inside the person's own settings folder. Copying the project copies its state,
and two people on the same folder do not fight over one file in one home directory.

Three plain text files:

| File | What is in it |
|---|---|
| `.unluminate/workspace.conf` | `explorer.visible`, `terminal.visible`, `terminal.tabs`, `files.active` |
| `.unluminate/open-files.txt` | one path a line, in tab order |
| `.unluminate/expanded-folders.txt` | one path a line |

The lists are files of their own rather than numbered names in the conf, because `recent.txt` already
does exactly that with a list of paths and because a list of paths reads better one to a line than as
`files.open.07 = ...`. The conf is the same `services::store::Values` format the settings file uses;
`Values::to_text_headed` is what lets it carry a heading of its own.

**Paths are written relative to the project** wherever they are inside it, so a project that is moved,
or checked out somewhere else on another machine, still opens the files it was left with.

**Terminals come back as fresh shells.** What a program was doing when the window closed is gone and
cannot be brought back; what is restored is the same number of shells in the project's folder, which
is what a person means by "my terminals were there".

**Reading and writing are turned on by the released binary only.** `UnluminateApp::restore_project` is
called from `main.rs` and by nothing else, exactly as `load_settings` is, so a test neither reads nor
writes a `.unluminate` folder — and a `.unluminate` written into a screenshot test's own sample project would
change what the explorer draws in the middle of a test.

**It is written when it changes, not every frame.** The window keeps what it last wrote and compares;
almost every frame writes nothing.

**A file that has since gone is left out** when the state is read, so a project never opens with a tab
pointing at nothing.

---

## 6. The tools moved into the title bar

The `F` button and the three Markdown view modes used to sit in a strip of their own, 44 points tall,
between the title bar and the tabs. The strip was drawn for a `.md` file and not for a `.rs` one — the
right rule, and `services::file_kind` still decides it — but the cost was that switching tabs moved
the tabs, the explorer and the whole editing area up and down by 44 points. A control that appears is
fine; a window that jumps is not.

They are now at the right hand end of the title bar, in front of the window buttons, and the strip is
gone. `size::TOOLBAR` went with it. The bar's height never changes, so nothing below it moves.

`components/toolbar.rs` is `components/text_tools.rs` now, because it is no longer a bar.
`title_bar::tools_rect` decides where they go, and the title bar uses the same function to stop its
own drawing and its own window-moving drag short of them, so nothing can run underneath a tool.

**The file name came out of the title bar.** It was centred, with its folder after it and an amber dot
for unsaved changes. All three are somewhere better: the name is on the file's own tab, the dot is on
the tab as well, and the folder was the project — which the bar now says once, after `Git`, rather
than repeating on every file.

---

## 7. A picture opens in a tab

`services/picture.rs` holds one, `components/picture_view.rs` draws it, and
`services::file_kind::is_image` decides which files are one: `png`, `jpg`, `jpeg`, `gif`, `bmp`, `ico`,
`webp`, `tif`, `tiff` — every format the `image` crate is built with here. `.svg` stays text: it is a
file you edit rather than one you look at.

**A tab still holds a `Document`.** `Document::at_path` makes an empty one that carries the path and
nothing else, so the tab is named after the file, the explorer marks the row as open, and the tab strip
needs no second kind of tab. `OpenFile::picture` is what tells them apart, and the window asks that
question in two places: what to draw in the editing area, and what to refuse to save. Saving a picture
would write an empty file over it, so `Save` and `Save As` say so and do nothing.

**It starts fitted, not at one to one.** A photograph four thousand pixels across opened into a window
nine hundred points wide, showing its top left corner, would be a picture viewer that has to be zoomed
out before it shows anything. A picture *smaller* than the area is left at its own size rather than
blown up, because a small icon stretched to fill the window is not what "fit" means to anybody.

**Zooming.** `Cmd/Ctrl` and plus or minus are the `View` menu's existing `Increase Font Size` and
`Decrease Font Size` entries, aimed at the picture when the open tab holds one. They are menu entries
rather than keys read in the component for the reason the whole menu exists: on macOS AppKit hands a
menu item's key equivalent to the menu before the window sees it, so a key read in a component would
work on Windows and be dead on macOS. `Reset Font Size` fits the picture back into the area.

A pinch and the wheel with the zoom modifier held both arrive as `input.zoom_delta()`, the same signal
the editor's own zoom uses, and are gathered rather than applied one at a time — a gesture is a stream
of multipliers a fraction over one, and a picture that jumps by a quarter on each of them is unusable.

**Two filters, not one.** Magnified, nearest neighbour, so a picture zoomed in far enough to see its
pixels shows its pixels. Minified — which is what a photograph fitted into the editing area is —
linear, because nearest neighbour throws away most of the rows and columns and leaves a large picture
ragged and speckled.

**A file that will not decode is a tab that says so**, with the decoder's own words, rather than a row
in the explorer that quietly does nothing when it is clicked. `is_image` answers from the extension
alone because the explorer asks it of every row in a folder; the decoder is what really knows.

---

## 8. What was checked, and how

The offscreen screenshot tests cover what can be rendered without a window: the rail's three buttons
and what they toggle, the eight grips by name, the tools being in the title bar and the editing area
not moving when they are absent, a picture opening and zooming, `Save` leaving a picture alone, and a
project's state going out to `.unluminate` and coming back.

Four of the seven cannot be proved that way, because they are the operating system's answer rather than
Unluminate's, so they were driven against the real `unluminate.exe` on Windows:

| What | What happened |
|---|---|
| Resize | Right edge 1100→1300, bottom 720→840, left edge, top-left corner, bottom-right corner — all moved the window |
| Pointer | I-beam over the writing, `sizewe` over the divider and the window's left edge, `sizens` top and bottom, `sizenwse` at the corners |
| Open Folder | A second `unluminate.exe` started on the chosen folder; the first window kept its project |
| Project state | Four tabs, an expanded folder and a terminal closed and reopened, all back in order |
| Picture zoom | 100% → 195% on three presses of Ctrl and plus; 195% → 88% on four notches of Ctrl and the wheel |
