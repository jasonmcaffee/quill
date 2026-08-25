# Implement: the improvements in tasks/improvements.md
 
> Source ask: work through `tasks/improvements.md` in full — move the font controls and the background
> opacity into a Settings modal, open any text file, make every pane draggable, allow several Quill
> instances with a recent projects list, add a terminal with tabs, put the menus in the macOS menu bar
> with Quill first, and break the code up into folders.
> Spec: `tasks/improvements.md`, plus the new terminal design document `tasks/quill-terminal-tdd.md`.

## Decisions made before writing code

- The Settings window is an in-window modal (`egui::Modal`), because `improvements.md` asks for a modal.
  It is laid out like the IntelliJ screenshot in `tasks/img.png`: a category list down the left, a
  breadcrumb heading, and the chosen page's sections on the right.
- `Settings -> Appearance -> Font` sets the editor's font family and size for the whole document rather
  than for the selection. A new `Document::set_base_style` in `quill-core` applies it without pushing an
  undo entry and without marking the file as having unsaved changes, because a display setting is not an
  edit.
- The terminal uses `alacritty_terminal` for the escape sequence emulation and its pseudoterminal, with
  the grid drawing, the tabs, the key encoding and the colour palette written here. The reasoning, the
  options considered and the numbers behind the choice are in `tasks/quill-terminal-tdd.md`.
- The macOS menu bar is built with `muda`. Both the macOS menu bar and the in-window menu bar used on
  Windows are built from one `MenuModel`, so there is one definition of what the menus hold. A test forces
  the in-window bar, because a test process has no application menu bar to look at.
- Several Quill windows means several processes, one project each, started with the folder as the first
  argument. `services/launcher.rs` records why.

## Steps

[x] ✅ - write the terminal technical design document `tasks/quill-terminal-tdd.md`: what a terminal has to
      do, the options considered with crates.io and GitHub numbers read today, the recommendation, the
      architecture, the key encoding, the colour palette, resizing, and the testing plan
[x] ✅ - reorganise `quill-app` into `app/`, `components/`, `services/` and `theme/`, moving every existing
      module into its folder and updating the imports and the tests, with no behaviour change
[x] ✅ - add `Document::set_base_style` to `quill-core` with unit tests: it changes the family and size over
      the whole document, leaves bold and colour alone, and does not mark the document as modified
[x] ✅ - add `services/store.rs`: the settings file and the recent projects list on disk, with unit tests
      against a temporary directory
[x] ✅ - add `settings.rs`: the settings model, its categories and pages, and how a change reaches the
      document and the window
[x] ✅ - add `components/settings_dialog.rs`: the modal with the category list on the left, the breadcrumb,
      the Font section and the Background section
[x] ✅ - take the font family and size, the background opacity control and the undo and redo buttons out of
      the toolbar; keep undo and redo on the keyboard only
[x] ✅ - add `components/splitter.rs`: one draggable divider used by every pane, with a hover and drag
      highlight and a minimum and maximum size
[x] ✅ - make the explorer width draggable, the side by side split draggable and the terminal height
      draggable, all through the splitter, and persist the widths
[x] ✅ - open any text file: `services/file_kind.rs` decides what is text, a file that is not `.md` opens as
      plain text, only files that are not text are dimmed, and the Open File picker offers every file
[x] ✅ - add `app/actions.rs` and `components/menu_bar.rs`: one `Action` enum, one list of menus, the
      in-window bar with `Quill` first and then `File`, `Edit`, `View`, and one dispatcher
[x] ✅ - add `services/native_menu.rs`: the macOS menu bar through `muda`, built from the same list of menus,
      with the in-window bar hidden on macOS
[x] ✅ - add `services/launcher.rs` and `File -> New Window`: a second Quill process on a folder of its own
[x] ✅ - add `File -> Recent Projects`: the list, opening one in a new window, and writing the list on every
      folder that is opened
[x] ✅ - new crate `quill-terminal`: the session over a pseudoterminal, the screen snapshot, the colour
      palette, the key encoding, the mouse reports and the tabs, with unit tests that need no window
[x] ✅ - add `components/terminal_panel.rs`: the bottom tile with tabs, the drawn grid, the cursor, the
      scrollback, selection and copy, and the resize that repaints
[x] ✅ - wire the terminal into the window: the View menu entry, the keyboard shortcut, focus moving between
      the editor and the terminal, and the panel height persisting
[x] ✅ - unit tests for `quill-terminal`: the key encoding, the palette, the screen snapshot after feeding
      escape sequences, resizing, the tab list, and two tests that run a real shell
[x] ✅ - font fallback in `services/text_renderer.rs`, found by looking at a screenshot: a character the
      chosen family has no shape for is drawn from another family, which is what a terminal has to do for
      the arrows and box drawing characters `claude` and `codex` use
[x] ✅ - screenshot tests for the new interface: the settings modal and its two pages, the toolbar without
      the moved controls, a wider explorer after a drag, a dragged preview split, a Rust file opened as
      plain text, the recent projects menu, the terminal with output, with colours, with a full screen
      program, with two tabs, and at two heights
[x] ✅ - add `examples/terminal_capture.rs` and use it to check `claude` and `codex` in the terminal, and the
      terminal after a resize, by looking at the images
[x] ✅ - run the whole test suite, accept the new snapshot images after looking at every one, and fix what
      broke
[x] ✅ - launch the real application: it starts with the macOS menu bar installed and the terminal open,
      writes its settings file and its recent projects list, and reports nothing on the error output.
      `quill --print-menus` prints what went into the menu bar. A screen capture of the bar itself is the one
      thing left for the user: `screencapture` returns a black image and AppleScript cannot read the menu bar
      without Screen Recording and Accessibility permission, which this session does not have
[x] ✅ - update `README.md`, `tasks/quill-technical-design-document.md` and add `CLAUDE.md` recording the
      conventions a later agent has to follow, including that every pane is draggable
[x] ✅ - validate-complete re-verification pass

## What the re-verification pass found and fixed

Reading `tasks/improvements.md` again from the start, against the code rather than against the plan, turned up
five things. All five are fixed.

1. **An opened file drew nothing.** The layout cache compared the document's revision number, and a revision
   counts changes to one document, so the next document opened could be at the same number and the last file's
   lines were kept. The screenshot of a Rust file being opened showed an empty editing area, which is what
   found it. `QuillApp::forget_layout` now throws the layout away when the document is replaced, and two tests
   assert that an opened file has been laid out.
2. **A missing glyph came out as an empty box.** `claude` draws arrows and box drawing characters that a text
   face does not have. Found by looking at the capture. `services::text_renderer` now looks in other families
   for a character the chosen one has no shape for, and the terminal is set in Menlo rather than Courier New,
   which covers far more of what a program draws its own screen with.
3. **A click in the macOS menu bar would not have woken the window.** eframe draws when something happens, and
   choosing something from the screen's own menu bar is not something the window receives, so the menu would
   have seemed dead until the pointer moved. The handler that takes the menu events now asks for a repaint.
4. **The explorer's filter box could not be typed into while the terminal was open**, because the terminal took
   every key press. The terminal now stands aside while a box that takes typing has the keyboard, and a test
   clicks the filter box with the terminal open and checks that what was typed reached it.
5. **A shell that would not start left an empty tile.** The tile hid itself as soon as it had no tabs, which
   included the case where it never had one, so the message saying why the shell would not start was never
   seen. The tile now closes only when a tab that existed has gone.

Two smaller things came from reading the ask again rather than from a fault: `Settings` is in the `Quill` menu
as well as the `Edit` menu, because `tasks/improvements.md` names both, and `File -> Open Folder in New Window`
was added, because "several instances, each with their own project" needs a way to open a project that has
never been open before without giving up the one that is open.

`sample/welcome.md` was also brought up to date: it told the reader to pick a font from the toolbar and to open
the opacity menu, and neither is there any more.

## What still needs a person

One thing: looking at the macOS menu bar. `screencapture` returns a black image and AppleScript cannot read a
menu bar without Screen Recording and Accessibility permission, neither of which this session has. Everything
that can be checked without seeing the screen has been: the application starts with the bar installed and
reports nothing on the error output, `quill --print-menus` prints what went into it, the process is named
`Quill` so the application menu says `Quill` rather than `quill`, and the same list of menus is clicked through
in the tests as the bar drawn inside the window.
