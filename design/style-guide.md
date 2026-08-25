# Quill's style guide

Read this before drawing anything new. It says how a control in Quill is built, so that a component
added today looks like the ones added a year ago rather than like a second design laid over the
first.

It is not a statement of taste. Almost everything here was read out of an image with a script rather
than chosen: `examples/sample_design.rs` reports, for each region of
`design/intial-design-screenshot.png`, the colour covering most of it and the most saturated colour
in it, and that is where the palette came from. Where a number has a different source, the source is
named.

## The baselines

An image in `design/` is the thing a new component is compared against, and a component is not
finished until it has one.

| Image | What it is the baseline for |
|---|---|
| `intial-design-screenshot.png` | The whole window: the palette, the bar heights, the explorer, the editing area. Every colour in `theme::color` was sampled from it. |
| `quill mac screenshot.png` | The running window on macOS, with the terminal open. What "it looks right" means. |
| `verification/live-window-over-desktop.png` | The window over a real desktop, which is the only way to see that the transparency works. |
| `verification/live-window-over-desktop-windows.png` | The same on Windows, over a backdrop of solid colours so that both halves of the requirement can be read off it: the background takes the colour behind it and every glyph stays solid. |
| `verification/terminal-claude.png`, `terminal-codex.png` | A full screen program running in the terminal, and the same after the tile was resized. |
| `components/*.png` | One capture per component added after this guide was written. |

`crates/quill-app/tests/snapshots` is a different thing and must not be confused with these. Those
are **accepted test output** — a change that alters the rendering fails against them. The images in
`design/` are **intent** — what the thing is meant to look like. A snapshot can be updated with
`UPDATE_SNAPSHOTS=1`; a baseline is changed only when the design changes.

## The palette is closed

`theme::color` is the whole list of colours Quill draws with. A new colour is added there, with a
comment saying which region of the design it was read from, or it is not used. Writing
`Color32::from_rgb(0x3A, 0x40, 0x4C)` at the point of use is how a window comes to have four slightly
different greys, and it must not happen.

| Name | Where it goes |
|---|---|
| `EDITOR` | Behind the text. The opacity setting's alpha is applied to this. |
| `TITLE_BAR` | The bar with the window buttons; also a modal's header. |
| `TOOLBAR` | The file tab strip, and the terminal tile. |
| `EXPLORER`, `EXPLORER_FOOTER` | The file list, and the strip counting the files. Also a modal's body and its list column. |
| `STATUS_BAR` | The bar along the very bottom. |
| `CONTROL`, `CONTROL_BORDER` | Inside a button or a dropdown that is not active, and the line round it. |
| `FIELD` | Inside anything typed into. |
| `DIVIDER` | Between two panels, and under a section heading. |
| `MENU` | Behind a menu. Darker than `CONTROL`, so a control drawn on a menu stands out. |
| `ACCENT` | Anything switched on: an active button, the caret, the underline on the open tab. |
| `SELECTED_ROW` | Behind the chosen row in any list. |
| `UNSAVED` | The amber dot meaning there are changes that have not been written. |
| `TEXT_STRONG` → `TEXT` → `TEXT_CONTROL` → `TEXT_DIM` → `TEXT_FAINT` | Five weights, brightest to faintest. A heading, body text, a control's label, a heading in the explorer, and placeholder text. |

There is one exception, and it is deliberate: **a syntax theme's token colours**. Those come from a
plugin, not from `theme::color`, because they are a property of the colour scheme rather than of
Quill. A syntax theme colours the tokens and nothing else — never the editing area's background,
because the window letting the desktop show through is what Quill is, and an opaque scheme takes it
away.

## The measurements are closed too

`theme::size` holds them, and everything is painted at an absolute position rather than through
egui's layout, because the window follows an image and the numbers come from that image.

| Constant | Points |
|---|---|
| `TITLE_BAR` | 50 |
| `ACTIVITY_BAR` | 36 (24 for the button, 6 either side) |
| `STATUS_BAR` | 32 |
| `EXPLORER` | 248 (150 to 620 by dragging) |
| `EXPLORER_FOOTER` | 28 |
| `ROW` | 28 |
| `INDENT` | 18 |
| `EDITOR_PADDING_X`, `EDITOR_PADDING_Y` | 43, 36 |
| `WINDOW_CORNER`, `CONTROL_CORNER` | 12, 6 |

Per-component numbers that more than one component needs live beside them: the terminal tile's header
is 32, a file tab strip is 32, a menu row is 24, and the window's own resize grip is 6 at the edges
and 16 at the corners.

There is no `TOOLBAR` height any more. The strip that held the text options and the Markdown view
modes was 44 points and was drawn only for a file its controls meant something for, so switching from
a `.md` file to a `.rs` one moved the tabs, the explorer and the editing area up and down by 44
points. `task-1658` moved those controls into the right hand end of the title bar, whose height never
changes. `color::TOOLBAR` stays: it is the fill behind the file tabs and the terminal tile.

**A row is 28 points.** In the explorer, in the settings list, in the git changes tree, in the plugin
list. A menu row is 24, and that is the only other row height in Quill. A third one is not added
without a reason written down.

## The shapes

**A row in a list.** The chosen row is a filled pill in `SELECTED_ROW`; hovering draws the same pill
in `CONTROL`. The pill is the row inset by 8 points horizontally and 1 vertically, corner radius 5.
Its text is `TEXT_STRONG` when chosen and `TEXT_CONTROL` otherwise. This is the open file in the
explorer, the chosen page in Settings, the chosen plugin, and the chosen commit in the history —
they are all the same drawing and they should be.

**A button.** `CONTROL` with a one point `CONTROL_BORDER` stroke, corner radius `CONTROL_CORNER`,
label `TEXT_STRONG` centred. Hovered it fills `ACCENT`. An icon button is a 22 point square with no
fill until it is hovered, when it fills `CONTROL` at corner radius 4.

**A dropdown.** `controls::dropdown`. Never `egui::ComboBox`, which brings its own styling.

**A flyout.** `controls::flyout`. An icon button that opens a panel of controls under itself and
stays open until the pointer goes elsewhere, which is what the title bar's `F` button is. It is a
sibling of the dropdown rather than a setting on it, because they are different things: a dropdown
shows a value and choosing one shuts it, a flyout is a panel used several times in a row. The panel
is `MENU` filled with a one point `CONTROL_BORDER` stroke, and inside it every row is named down the
left in `TEXT_DIM` at 11.5 points with its controls in a column at 78 points in. **A flyout must not
hold a dropdown or another flyout**: egui keeps at most one popup open at a time, so opening the
second shuts the first.

**A button carrying a word.** `controls::choice_button`. `ACCENT` filled when what it stands for is
on, `CONTROL` while the pointer is over it, and a one point `CONTROL_BORDER` outline otherwise, with
the word in `TEXT_STRONG` or `TEXT_CONTROL` at 12.5 points. The three line spacings are the only ones,
and they are buttons rather than a dropdown because they live in a flyout — and because which spacing
is on can then be seen without opening anything, as it can for the alignments beside them.

**A text field.** `FIELD` with a one point `DIVIDER` stroke, corner radius `CONTROL_CORNER`, an
`egui::TextEdit` with `Frame::NONE` inside it, text `TEXT_CONTROL` and placeholder `TEXT_FAINT`. A
search field has a magnifier at its left, 13 points in from the edge. **The box inside it is given
one text row, centred, through `controls::field_text_rect`** — never the whole height of the field.
egui lays a `TextEdit` out at the top of the rectangle it is given and `Frame::NONE` leaves no margin
to push it down with, so a field that hands over its whole height puts its words against its top edge,
on a different line from the magnifier. That was true of all five fields in Quill until `task-1657`.

**A menu.** `MENU` filled, one point `CONTROL_BORDER` stroke, 6 point inner margin, 340 points wide —
wide enough that `Open Folder in New Window` and `Cmd+Option+O` do not meet in the middle, which they
did at 260. A row is 24 points: a tick at the left when it is on, the name at 18 points in, the
shortcut right aligned in `TEXT_FAINT`. A row that cannot be used is drawn in
`TEXT_FAINT.gamma_multiply(0.6)` and takes no clicks. Every menu — the bar's, the explorer's context
menu, the gutter's — goes through `controls::menu_rows`, so they cannot drift apart.

**A modal.** `components::modal::show`, always — never `egui::Modal` directly. Filled `EXPLORER`
over a `from_black_alpha(120)` backdrop, a one point `CONTROL_BORDER` stroke, corner radius 10. A 46
point header filled `TITLE_BAR` with the title at the left in `TEXT_STRONG` and a close cross at the
right. A 52 point footer with a `DIVIDER` line along its top and the buttons at its right. The
Settings window is the reference; the commit panel, the git dialogs, the prompt, the confirmation,
`Go to File` and `Find in Files` are all built the same way.

**A modal is dragged by its header and resized from any of its edges**, and neither is a dialog's
business: both are in `modal::show`, so a dialog written later has them without asking. A double
click on the header puts it back in the middle at the size it asked for, exactly as a double click
on a pane divider does. Its geometry lives in egui's own memory under the modal's id — the window
has no decision to make about it and nothing goes to disk. Two rules about ordering, both the same
rule the window's own grips follow: the drag strip is added **before** the contents so the close
cross sits over it, and the eight grips are added **after** them so a list at the modal's edge
cannot take a drag meant for the edge.

**A search modal.** `Go to File` and `Find in Files`. A `controls::search_field` across the top of
the body, the list under it, and the count of what was found in the footer at the left in
`TEXT_FAINT` at 11 points opposite the button. The letters a query matched are picked out in
`ACCENT` through `controls::marked_text`; nothing is emboldened to show a match, because the accent
colour already says it and two signals for one thing is one too many. A row is chosen by a single
click and opened by a double click or by Enter, which is what IntelliJ does and what `task-1659`
asks for.

**A section heading inside a page.** The name in `TEXT_STRONG` at 12.5 points, then a `DIVIDER` rule
running to the right margin, as IntelliJ draws one. `settings_dialog::section`.

**A rail button.** `components::activity_bar`. A 24 point square in the 36 point rail down the far
left, inset 6 from its left edge — which is exactly what `components::resize_edges` takes, so a button
and the window's own resize grip never want the same point. On, it is the selection pill in
`SELECTED_ROW` with the icon in `TEXT_STRONG`; hovered, the same pill in `CONTROL`; otherwise no fill
and the icon in `TEXT_DIM`. Not a filled `ACCENT` square: a pane being open is a state rather than a
press, and three bright blue squares in a rail that is nearly always in that state would be the
loudest thing in the window.

**A grip on the window's own edge.** `components::resize_edges`, and nothing is painted — the window
already has its rounded rectangle, and a visible frame is what turning the decorations off was for.
Four edges 6 points wide and four corners 16 points square, each setting the pointer for the direction
it moves. **They are added to the `Ui` last**, after every pane, for the reason a divider is added
after the panes either side of it.

**A divider between panes.** `components::splitter`, always. It decides the grab width, the highlight
under the pointer, the pointer shape and the double click that puts the pane back. A new pane adds
its size to `settings::Panes`, with a smallest and a largest, clamped both when reading the file and
when dragging — and the divider is added to the `Ui` **after** the panes either side of it, or the
pane's own drag area sits on top of it and it never gets the drag.

## Type

There is one type scale, and a new number is not invented.

| Points | Used for |
|---|---|
| 13.5 | A modal's breadcrumb. |
| 13.0 | A modal's title. |
| 12.5 | Everything ordinary: a menu row, a list row, a control's label, a section heading. |
| 12.0 | A terminal tab, a tile's heading. |
| 11.5 | A shortcut in a menu, a line of explanation, a hint. |
| 11.0 | A heading inside a menu. |
| 10.5 | The explorer's heading and its footer counts. |

`theme::BOLD_FAMILY` is the real bold face, installed by `theme::install_fonts`. egui's built-in
fonts have no bold face and its `strong` styling only brightens the colour, so anything that must
actually be bold asks for that family. It is bound in `QuillApp::prepare`, before the first frame,
because asking egui for a family it has not been given panics.

British spelling in prose, and the American spelling where a name in the code already uses it, such
as `color` in `egui`.

## Icons are drawn

`theme::icon` holds one function per icon, taking a painter, a centre and a colour. Nothing is a
letter or a Unicode symbol: egui's default fonts have no glyph for most of them, and an absent glyph
renders as an empty box. That was found the hard way — the shift symbol at U+21E7 came out as a box,
which is why menu shortcuts are spelled in words.

An icon is drawn inside about a 10 point square around its centre, at a 1.3 to 1.6 point stroke.

**A letter shaped icon is drawn too**, and `theme::icon::font` — the `F` on the title bar's text
options button — is the one there is. It is three strokes, not the letter `F` set in a font and not
a picture. A picture cannot be tinted, and every icon in Quill is tinted where it is used: `TEXT_DIM`
sitting there, `TEXT_STRONG` when what it opens is open. Nor can a picture be drawn at another scale
without resampling it. `task-1657` offered to have an image generated for this one, and this is why
it was refused. Setting it as text is no better: the text options panel's `B` is real text, and it
needs `theme::BOLD_FAMILY` bound before the first frame to look like anything.

`task-1658` asked the same question again for the three icons in the activity bar and the answer is
the same, for a reason its own rail makes plain: each of those three is drawn in `TEXT_DIM` sitting
there, `TEXT_STRONG` when its pane is open and `TEXT_FAINT` when it cannot be used. A picture can
carry one of those three, and only at the size it was made. If a drawn icon is not liked, the answer
is to draw it better.

## Every control has a name

`response.widget_info` with plain wording: `Save`, `Bold`, `Resize explorer`, `Terminal tab: claude`,
`Commit`, `Annotate with Git Blame`. Three rules:

1. **The name is the plain wording**, with no tick, no padding and no decoration in it, so a test can
   ask for `Open Folder` however the row happens to be drawn.
2. **No two controls in one window share a name.** The Settings window's footer button says `Done`
   rather than `Close` because the window already has a `Close` button, and two controls with one
   name cannot be told apart by a test or by anyone reading them out.
3. **A control with no name cannot be tested**, because the screenshot tests find controls by name
   rather than by position. That is also what stops a control moving by a few points from breaking a
   test.

## What a component is

A function that takes a `Ui` and the rectangle it fills, draws itself, and returns what the user did:

```rust
pub fn show(ui: &mut Ui, area: Rect, /* what it needs to draw */) -> Outcome
```

It does not change the document, start a git command, install a plugin or write a setting. The state
changes in `app`, in one place, so two components cannot disagree about what happened and a component
can be drawn by a test with nothing behind it.

**A control is drawn only when it means something for the file that is open.** The `F` button is not
drawn at all for a `.rs` file or a picture, and the three view mode buttons are not drawn for a `.txt`
one. They sit at the right hand end of the title bar, whose height does not change, so a file with no
tools leaves that room empty rather than moving everything below it. The questions are asked of
`services::file_kind` — `formatting_applies` and `preview_applies` — so the tools and the View menu
cannot come to different answers, and a file kind is decided in one place. Dimming is for a control
that could be used in a moment, such as undo with nothing yet to undo; a control that can never apply
to this file is absent.

Everything a menu or a shortcut can ask for is an `app::actions::Action` with exactly one arm in
`QuillApp::run_action`. There are three menus now — the macOS bar, the bar Quill draws in its own
title bar, and the explorer's context menu — and they are all built from the same lists, so `Commit`
means one thing.

## Before it is called finished

- It uses only `theme::color` and `theme::size`.
- Its rows are 28 points, or its menu rows are 24.
- Its selection is the pill.
- Every control in it has a name, and no two names collide.
- It has a baseline in `design/components/`, and someone has opened the image and looked at it.
- It has a screenshot test, accepted into the platform's own folder, and the accepted image was
  opened before it was accepted.
- Anything it resizes goes through `components::splitter`, and remembers its size in
  `settings::Panes`.
