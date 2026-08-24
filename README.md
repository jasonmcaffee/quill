# Quill

A text editor for macOS and Windows, written in Rust. It opens `.md` and `.txt` files, has a file
explorer with folders that expand in place, and lets the desktop show through its background while the
text stays solid.

## Running it

```
cargo run --release -- sample/welcome.md
```

The argument is a folder to show in the explorer, or a `.md` or `.txt` file to open, in which case the
explorer shows the folder that file is in. With no argument the explorer shows the current directory.

`--opacity N` sets the starting background opacity between 0.05 and 1.0, and `--view raw|side|preview`
chooses which of the three ways of looking at the file it starts on. Both are the same settings the toolbar
changes, and they exist so a starting state can be chosen without clicking, which is what makes it possible
to capture the window in a particular state.

## What the window looks like

The window follows `design/intial-design-screenshot.png`: a title bar Quill draws itself with the three window buttons and the
file name centred, the formatting toolbar, the file explorer down the left with a filter box, the editing
area, and a status bar. The palette was read out of the design image rather than chosen by eye; run
`cargo run --example sample_design` to print the colour of each region of that image.

`crates/quill-app/tests/snapshots/design_comparison.png` is the same window set up the way the design shows
it, for putting the two side by side.

`design/verification/live-window-over-desktop.png` is a capture of the running window over a real desktop.
The wallpaper is visible through the explorer, the editing area and the status bar, and every piece of text
is solid on top of it, which is what the opacity control is for.

## What it does

The `File` menu in the title bar holds `Open Folder`, `Open File`, `Save` and `Save As`, with
`Cmd+Shift+O`, `Cmd+O`, `Cmd+S` and `Cmd+Shift+S` as shortcuts. On Windows the control key takes the place
of the command key. The pickers are the operating system's own.

Three buttons in the toolbar, immediately to the left of undo, switch between three ways of looking at a
Markdown file: the raw source, the source and the preview side by side, and the preview on its own. The
preview is read only, and it follows the source as it is edited.

Editing, in the modes that show the source: select with the mouse or with shift and an arrow key, cut, copy, paste, move the caret by
character, by word, to the start or end of a line, and to the start or end of the document, undo and
redo.

Character formatting: font family, size, bold, italic, underline, strikethrough and colour.

Paragraph formatting: left, centre, right and justified alignment, and single, one and a half or double
line spacing.

The window: a file explorer on the left nested to any depth listing every file, with the ones Quill cannot
open dimmed, and a box that filters the file list, a
formatting toolbar along the top, a status bar showing the file, its kind and the line and column of the
caret, and an opacity control that fades the background so the desktop behind Quill is visible. The
explorer can be put away with the button in its heading.

Keyboard: command plus B, I or U for bold, italic and underline. Command plus shift plus X for
strikethrough. Command plus L, E, R or J for the four alignments. Command plus A to select all, command
plus Z to undo, command plus shift plus Z to redo, command plus S to save. On Windows the control key
takes the place of the command key.

## How it is put together

Two crates.

`crates/quill-core` is the editor. It holds the text buffer, the formatting, the caret, layout, undo and the
Markdown parser, and it has no user interface dependencies at all, so its tests run with no window, no
graphics card and no fonts. Its only dependency is `unicode-segmentation`.

The Markdown parser is worth a note. It does not draw anything. It reads the source and produces the same
three things a document holds, a rope of text with character spans over it and one paragraph setting per
line, so the preview is drawn by the ordinary layout engine and the ordinary painter. Nothing in the window
knows how to render Markdown.

`crates/quill-app` is the window. It uses `eframe` and `egui` for the window, the input events, the
graphics device and the toolbar controls, `fontdb` to find installed fonts and `ab_glyph` to read and
rasterise them. It implements the `FontMetrics` trait that `quill-core` measures text through.

The text buffer, the line breaking, the alignment, the hit testing and the glyph atlas are written here
rather than taken from a library. `tasks/quill-technical-design-document.md` records why, which other
options were considered, and what was read while writing it.

## Tests

```
cargo test
```

Three layers, 188 tests.

`quill-core` has 119 unit tests, including 24 for the Markdown parser and a randomised comparison of the rope against a plain `String`
over 1500 edits with the tree invariants checked after every one. Layout tests measure through a fixed
width stub, so their expected numbers are arithmetic a reader can check and are the same on every
machine.

`quill-app` has 23 unit tests covering the file explorer, its filter, and real font measurement and glyph
packing.

`crates/quill-app/tests/screenshots.rs` has 46 tests that build the whole application, feed it real
events, render it through `wgpu` and write a PNG for each one to `crates/quill-app/tests/snapshots`.
Those images are meant to be looked at: they are how a person or an agent confirms that bold text is
bolder and that centred text is centred. Once accepted they are also the comparison baseline, so a later
change that alters the rendering fails a test.

To accept new images after a deliberate change:

```
UPDATE_SNAPSHOTS=1 cargo test
```

A run that differs writes `{name}.new.png` and `{name}.diff.png` next to the accepted image.

## Not included

Right to left and complex writing systems. Version one places one grapheme cluster after another from
left to right, which is correct for Latin, Greek and Cyrillic and wrong for Arabic and Hindi. The
`FontMetrics` boundary is where a shaping step would go.

Search and replace, several carets at once, tabbed documents, and a settings file. The background opacity
and the view mode reset when Quill restarts.

In the Markdown preview: tables, footnotes, images shown as pictures rather than as their text, reference
style links, nested block quotes and HTML. Tables need layout Quill does not have; the rest are rare in
prose.
