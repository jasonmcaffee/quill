# Working on Quill

Read this before changing anything. It records the conventions the code already follows, so that a later
change looks like the rest of the code rather than like a second style laid over it.

## What the crates are for

| Crate | What is in it | What must never be in it |
|---|---|---|
| `quill-core` | The editor: the text buffer, the character and paragraph formatting, the caret, layout, undo, the Markdown parser and the syntax tokeniser. | Any user interface dependency. Its tests run with no window, no graphics card and no fonts. |
| `quill-terminal` | The terminal: the session over a pseudoterminal, the screen the painter reads, the colour palette, the key encoding and the mouse reports. | Any user interface dependency, for the same reason. |
| `quill-git` | Reading and changing a git repository: the status, blame, the log, diffs, branches, and every operation on the Git menu, plus the thread they run on. | Any user interface dependency, and any decision about what a dialog looks like. Its tests build real repositories in a temporary folder and ask git what happened. |
| `quill-app` | The window: drawing, input, real fonts, the settings on disk, the menus, and the plugin registry. | Editor behaviour, terminal emulation or git plumbing. Those belong in the crates above. |

`quill-app` is laid out in four folders and a new file belongs in one of them:

- `app/` — the window's own state, and `app/actions.rs`, which is what the menus and the keyboard ask for.
- `components/` — drawing. One file for each piece of the window.
- `services/` — everything that is not drawing: the file tree, the fonts and the glyph atlas, the settings
  and recent projects on disk, starting a second window, the macOS menu bar, and what Windows needs
  before the desktop will show through the window.
- `theme/` — the palette, the measurements and the drawn icons.

## The look is written down, and a new control is measured against it

`design/style-guide.md` says what a control in Quill is built from: the palette is closed, a list row
is 28 points and a menu row is 24, selection is one pill drawn one way, a modal has one shape, icons
are drawn rather than lettered, and every control has a plain name. **Read it before drawing anything
new**, and add to it rather than inventing a second answer.

Two things it separates that are easy to confuse. `design/` holds **intent** — the image a component
is compared against, changed only when the design changes. `crates/quill-app/tests/snapshots` holds
**accepted output** — a change that alters the rendering fails against it, and `UPDATE_SNAPSHOTS=1`
accepts a new one after somebody has opened the image and looked at it.

`components::modal` is the furniture every modal is made of: the frame, the header, the body
rectangle, the footer, the buttons, the rows, a field and a tick box. The Settings window, the commit
panel, the git dialogs, the text prompt and the confirmation are all built from it. A tenth modal
that draws its own header would be a tenth modal that almost agrees with the other nine.

`components::controls` is the same idea for the small things: the dropdown, the flyout, a menu row, a
button with a word on it, an icon button, and `field_text_rect` — the rectangle a field hands to the
`egui::TextEdit` inside it. That last one exists because all five fields in Quill had the same fault:
egui lays a text box out at the top of the rectangle it is given, `Frame::NONE` leaves no margin to
push it down, and a field that handed over its whole height put its words against its top edge. One
function is what stops a sixth field being the sixth chance to get it wrong.

**A flyout must not hold a dropdown or another flyout.** egui keeps at most one popup open at a time,
so opening the second shuts the first — which is why the three line spacings in the text options
panel are three buttons rather than the dropdown they used to be.

## Git runs the `git` program, on a thread

`quill-git` shells out to `git` rather than using a library, and the reason is what the machine's own
git already knows: a credential helper, an ssh agent, `commit.gpgsign`, hooks, `safe.directory`, an
identity for this repository in particular. A push from Quill has to be the same push you get in the
terminal. The cost is that the output has to be read, which is answered by asking for the formats git
provides for being read — `--porcelain=v2 -z`, `--line-porcelain`, `--format` with the record
separators — never the ones meant for a person.

Two rules follow, and a change should keep both.

**Nothing invents an error message.** Every call returns git's own standard output and standard error
whether it worked or not, and that is what the status bar shows. A rejected push, a merge conflict, a
detached HEAD and a missing upstream all explain themselves better than Quill could.

**Every command goes through `quill_git::Worker`**, which runs one at a time on a thread. Not because
the window would be slow — because it would stop drawing until git finished, which on a fetch looks
exactly like a crash. One at a time, because two commands at once in one repository fight over
`index.lock`.

## A control is absent when it cannot apply, not dimmed

The formatting strip is not drawn above a `.rs` file, and the three view mode buttons are not drawn
above a `.txt` one. Quill saves plain text and carries no formatting to disk, so everything in that
strip is about how prose is *shown*, and above a source file it is fourteen controls that mean
nothing — the three view modes offering the Markdown parser's reading of a file that was never
Markdown. The window gives the forty four points to the editing area instead.

Two functions in `services::file_kind` answer it, and nothing else does: `formatting_applies` and
`preview_applies`. The toolbar and the `View` menu both ask them, so they cannot come to different
answers about the same file, and a file kind stays decided in one place.

Dimming means something different and is still right where it was: a control that could be used in a
moment, such as undo with nothing yet to undo, or the whole Git menu outside a repository. A control
that can never apply to this file is absent.

## The editor's font is one setting, and it reaches every tab

The family and the size in `Edit -> Settings -> Appearance -> Font` are one setting for the whole
window, the way IntelliJ has one editor font. `QuillApp::set_the_font_everywhere` is what puts it
into effect, and every path that changes it goes through that one function — the dialog, the
keyboard's command and plus, a trackpad pinch, and reading the settings file at startup. It used to
reach `document_mut()` alone, so opening three files and then changing the font left two of them in
the old one until Quill was restarted.

Setting it is **not an edit**: nothing goes onto any document's undo history and no file is marked as
having unsaved changes, because what Quill saves is plain text and carries no formatting.

**egui's own keyboard zoom is switched off**, in `theme::apply`. It scales the whole interface, menus
and all, which is a browser's zoom rather than an editor's; with it left on, one press of command and
plus would do both.

## Every pane is resized by dragging its edge

The explorer, the split between the Markdown source and its preview, and the terminal tile are all resized
by dragging, and they all go through `components/splitter.rs`. **A new pane must use it too.** The grab
width, the highlight while the pointer is over it, the pointer shape and the double click that puts the pane
back to its usual size are decided in that one file, so every divider in Quill behaves the same way.

Two things to know when adding one:

- The divider has to be added to the `Ui` **after** the panes either side of it. The editing area takes
  drags over the whole of its rectangle, and the divider overlaps its edge, so a divider added earlier sits
  underneath and never gets the drag. This was a real fault, found by a test that dragged and saw nothing
  move.
- Its size belongs in `settings::Panes`, which is written to the settings file, so the pane is where it was
  left next time Quill starts. Give it a smallest and a largest size and clamp both when reading the file
  and when dragging.

## One action, one place

Everything a menu or a keyboard shortcut can ask for is an `app::actions::Action`, and
`QuillApp::run_action` is the only place an action turns into a change. There are two menu bars, the one
macOS draws along the top of the screen and the one Quill draws inside its own title bar on Windows, and
both are built from `app::actions::menus`. Adding an entry means adding a variant, an entry in that list and
an arm in `run_action`, and both bars get it.

Do not read the keyboard for something that is also a menu entry. On macOS a shortcut on a menu item is a
key equivalent, and AppKit hands it to the menu before the window sees it, so the key press never reaches
egui. Cut, copy and paste are the exception, marked in the list as not coming from the keyboard, because
the platform delivers those as clipboard events.

## Components take a rectangle and return what happened

A component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and returns what
the user did in it. It does not change the document or the window's state. The state changes in `app`, so
two components cannot disagree about what happened, and a component can be drawn by a test without a
document behind it.

Everything is painted at an absolute position rather than through egui's layout, because the window follows
`design/intial-design-screenshot.png` and the measurements come from that image. `theme::size` holds them.

## Give every control a name

Every control calls `response.widget_info` with a plain name: `Save`, `Bold`, `Resize explorer`,
`Terminal tab: claude`. The screenshot tests find controls by name rather than by position, so a control
that moves does not break a test, and a control with no name cannot be tested at all. Two controls must not
share a name: the Settings window's button says `Done` rather than `Close` because the window already has a
`Close` button.

## The desktop shows through, and Windows needs three things for it that macOS does not

The window is created with `with_transparent(true)`, the background is painted with the opacity
setting's alpha and every glyph is painted at alpha 255. On macOS that is the whole story.

On Windows the same code drew a solid window, and `services/windows_transparency.rs` is what fixes
it. Three separate faults, each of which alone is enough to leave the window opaque: wgpu picked
Vulkan, whose surface offers no transparent composite mode, so DX12 is named; a DXGI swapchain built
from a window handle can only be `Opaque`, so it is built from a DirectComposition visual instead;
and the window's redirection surface is never cleared by winit, so it holds undefined bytes that
composite as opaque white, and it is filled with black — which is how GDI writes a zero alpha — once
a frame. **Filling it once does not work**, because eframe keeps the window hidden until it has
painted its first frame, so the fill lands before Windows has allocated the surface. Section 9.2 of
`tasks/quill-technical-design-document.md` records how each was measured and what was rejected;
`design/verification/live-window-over-desktop-windows.png` is what it should look like.

## Shipping it is a folder of its own

`installer/` turns the built binary into something a person can install, and it does not reach into the
application: it packages what `cargo build --release` already produces. `installer/icon` draws the mark
once and writes `quill.ico`, `quill.icns` and an iconset; `installer/windows` compiles an Inno Setup
script into a single setup exe; `installer/macos` builds `Quill.app` and a disk image. Each has a build
script that goes from a checkout to a file that can be handed to somebody, so nothing about releasing
lives only in a person's memory.

The one place it does reach in is `crates/quill-app/build.rs`, which puts the icon and a version block
inside `quill.exe`. That has to be inside the executable — Windows reads the taskbar icon, the Alt-Tab
entry and the Add or Remove Programs version from there, and no installer can supply them from the
outside. A missing Windows SDK makes it a warning rather than an error, so a build with no `rc.exe`
still produces a working, if unlabelled, `quill.exe`.

**The version lives in `Cargo.toml` and nowhere else.** It reaches the resource block, the installer's
file name, the Add or Remove Programs entry and `Info.plist` from there. Do not write it down a second
time.

## Tests

Four layers, and a change should leave all four green:

1. `quill-core` and `quill-terminal`: unit tests with no window. Layout tests measure through a fixed width
   stub, so the expected numbers are arithmetic a reader can check and are the same on every machine.
2. `quill-app`: unit tests for the file tree, the fonts, the settings file, the menus and the key encoding.
3. `crates/quill-app/tests/screenshots.rs`: builds the whole window through `egui_kittest`, feeds it real
   events, renders through `wgpu` and writes a PNG for each test. **Look at the images.** They are how a
   person or an agent confirms that bold text is bolder and that the terminal's colours are right. Once
   accepted they are the comparison baseline, so a later change that alters the rendering fails a test.
   `UPDATE_SNAPSHOTS=1 cargo test` accepts new images, and nothing should be accepted without opening it.
   Each platform has its own accepted set — macOS reads `tests/snapshots`, Windows `tests/snapshots/windows`
   — because the menus, the window buttons and the font are all deliberately different there, so one set
   cannot be the baseline for both. `shot()` at the top of the test file is where that is decided.
4. The real application: `cargo run --release`, and `cargo run --example terminal_capture -- claude` for the
   terminal. For git, `pwsh tools/build-git-demo.ps1` builds a small repository under the temporary
   folder — three commits by three authors on three widely separated dates, a branch, an uncommitted
   change and an untracked file — which is enough to exercise every entry on the Git menu by hand. Layer 3 renders through the same code but offscreen, so only a real run shows that the
   operating system honoured the window's transparency or drew the menu bar.

A screenshot test must be the same on every run. The terminal's screenshot tests feed fixed bytes to a
session with no shell behind it, through `QuillApp::new_detached_terminal_tab`, because when a real shell
answers is not something a test can know. Tests that do run a shell assert on text and wait with a timeout.

Three rules the screenshot tests follow, all of them the answer to a test failing for a reason that was
not a fault in Quill (`task-1654` — the file's own comments carry the detail):

- **Nothing builds a graphics device of its own.** `egui_kittest`'s `.wgpu()` builds a new instance,
  adapter and device for each harness, and ninety one of those built and torn down across as many
  threads as the machine has killed the process with an access violation on about one run in nine.
  A small pool is built once at first use and shared, and every harness comes from `builder()` in the
  test file rather than `Harness::builder()`, so a test added later cannot go back to one of its own.
- **A fixture two tests share is written once.** `sample_folder()` used to write its files on every
  call, so one test could read a file another test had truncated a moment ago and not yet filled in.
  It is built behind a `OnceLock` now. A fixture only one test uses, like `git_folder(name)`, may be
  written each time — the name is what keeps them apart.
- **A loop that waits calls `pump`, not `Harness::run`.** `run` gives the window four steps to go quiet
  and panics otherwise, which is right for a settled window and wrong while git or an image is still
  being worked on. Running out of steps inside one attempt is not a failure; running out of attempts
  is, and the loop says so.

Tests must not read or write the settings of the person running them. `QuillApp::new` reads nothing; the
released binary calls `load_settings` and a test that wants a store calls `use_store` with a folder of its
own.

## Writing

Plain sentences. Say what the code does and why a decision was made, once. Every module has a comment at
the top saying what it is for, and a decision that a reader might disagree with is recorded where it was
made rather than left to be rediscovered. There are examples throughout: why undo restores a saved state
instead of an inverse, why the terminal's lock is held for the copy and not for the drawing, why a second
Quill window is a second process.

British spelling in prose, and the American spelling where a name in the code already uses it, such as
`color` in `egui`.

## Plugins are data, and nothing in one is executed

A plugin is a folder with a `plugin.conf` and an icon in it, read by the same value store the
settings file uses. It describes a language: its extensions, its keywords, what a comment and a
string look like, and a colour per kind of token. **Nothing is run.** A dynamic library would mean an
unstable interface across a `dlopen` boundary and a plugin crash taking the editor with it;
WebAssembly answers both and costs a runtime and a host interface that has to be designed first.

`plugin.kind` is the seam a later version widens, and it is checked: a manifest asking for anything
but `language` is refused with a message rather than half-loaded. Do not quietly widen it.

A colour scheme **colours the tokens and not the editing area**. The window letting the desktop show
through is the whole character of the product, and a scheme that repaints the background opaque would
trade that away to be a shade nearer a screenshot.

## The documents

- `README.md` — what Quill is and how to run it.
- `documentation/overview.md` — what Quill looks like: a capture of each part of the window, over a real desktop.
- `design/style-guide.md` — how a control in Quill is built, and what the baselines are.
- `tasks/quill-ide-tdd.md` — the line numbers, the tabs, the explorer's menu, git and the plugins:
  what was chosen, what was rejected and why.
- `tasks/quill-technical-design-document.md` — the editor: the options that were considered, what was
  chosen and why, and what is deliberately not included.
- `tasks/quill-terminal-tdd.md` — the same for the terminal.
- `tasks/task-1657-text-options-tdd.md` — the `F` button and its flyout, the font becoming one
  setting for the window, zooming with a pinch and with the keyboard, and the two drawing faults
  that were fixed alongside them.
- `tasks/quill-installer-tdd.md` — how Quill is delivered: the icon, the Windows installer and the
  macOS bundle, and the options that were weighed for each.
- `installer/README.md` — how to build an installer, on either platform.
- `tasks/improvements.md` — the ask that the settings window, the panes, the terminal and the menus came
  from.

Each document stands on its own. If a fact from another one is needed, state the fact.
