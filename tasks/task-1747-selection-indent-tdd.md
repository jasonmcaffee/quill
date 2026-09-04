# task-1747 — Tab and Space indent the selection, and the panel header keeps the normal cursor

## 1. Introduction

Two things a person does every day in an editor, and Unluminate answered both the way a text field
answers them. Select a block of lines and press `Tab`, and the block is **replaced by one tab
character**; press `Space`, and it is replaced by one space. Every other editor indents the block
instead, which is what a person means by "indent", and what the ticket asks for.

The second thing is small: hovering the pointer over the top of a panel — the header that is also
the drag handle `task-1697` made — turns the cursor into a grabbing hand. The ticket asks for the
normal cursor there.

## 2. Goals and non-goals

### Goals

- With a selection, `Tab` puts one tab at the start of every line the selection touches, and the
  selection stays over the text it covered.
- With a selection, `Space` does the same with one space.
- With no selection, both keys keep doing exactly what they do now: typing the character at the
  caret, inside the current run of typing.
- One key press is one undo step, whatever the selection spans.
- The same two edits are reachable from the command line as `unluminate-cli editor indent`, with and
  without `--space`, and documented in `unluminate-cli/docs/commands.md`.
- The panel header's hover and drag no longer change the cursor; the pointer stays the normal
  arrow, and the blue drop zones remain the feedback while a panel is carried.
- Tests at every layer: the core rules with no window, the window's key handling and the cursor in
  the screenshot tests, and the command line through the catalogue tests that already exist.

### Non-goals

- **Outdent.** `Shift+Tab` in every surveyed editor removes one level of indentation, and it is the
  natural half of this feature. It is not asked for, and it has its own awkward cases — a line with
  no indentation, a line indented with spaces being outdented by a tab, a selection whose first line
  is only half selected — each of which needs a decision the ticket does not give. Until it is asked
  for, `Shift+Tab` indents the same way `Tab` does, which is strictly better than the current
  behaviour of throwing the selection away, and undo is the way back.
- **An indent setting.** Unluminate has no tab width and no "insert spaces for tabs" preference, and the
  ticket asks for a tab from `Tab` and a space from `Space`, which is the one answer that needs no
  setting. A document Unluminate saves is plain text, and what the key says is what goes in.
- **Auto-indent on `Enter`.** A new line taking the indentation of the line above is a different
  feature with its own rules, and it is not asked for.
- **Block (column) selections.** Unluminate has no rectangular selection, so there is nothing to answer
  here.

## 3. Problem statement

`components::editor_view::handle_input` turns a bare `Tab` into `Command::Insert("\t")`, and a
`Space` arrives as `Event::Text(" ")` and becomes `Command::Insert(" ")`. `Document::insert` opens
with `if !range.is_empty() { self.remove_range(range) }` — the selection is deleted and the
character is put in its place. That is the right answer for a single caret and the wrong one for a
selection, and the command has no way to tell the two apart, because the distinction is not in the
text being inserted: it is in what the selection is for.

The cursor half is two lines in `components::dock::handle`: `CursorIcon::Grab` while the pointer is
over the header and `CursorIcon::Grabbing` while a panel is being carried. The header covers the
whole top of a panel, so the hand appears a great deal in ordinary use, and the ticket does not
want it.

## 4. Research and precedent

The three editors a person would compare this against all agree on the shape:

- **IntelliJ IDEA** indents the selected block with `Tab` and outdents it with `Shift+Tab`; with no
  selection, `Tab` inserts the indent at the caret. The block keeps its selection, so pressing
  again indents it again.
- **VS Code** does the same: `Tab` over a multi-line selection indents every line of it by the tab
  size, `Shift+Tab` undoes one level, and `Space` over a selection indents by one space per line.
  With no selection both keys type at the caret.
- **Sublime Text** and **Notepad++** follow the same rule: `Tab` indents the lines, `Shift+Tab`
  outdents, and a bare key at a bare caret inserts.

The one rule all four share, and the one this design keeps, is that **the selection is what makes
the key an indent rather than a type**. The character the key names is what the indent is made of —
a tab from `Tab`, a space from `Space` — which is also what Unluminate needs no setting to answer.

What the surveyed editors do not agree on is what the indent is *made of*: IntelliJ and VS Code
insert the configured tab size, in tabs or in spaces, and VS Code even has a setting for how `Tab`
behaves. Unluminate has no such setting and no reason to grow one for this: the ticket names the two
characters, and a literal tab and a literal space are the only answers that cannot be wrong on a
machine that has not told Unluminate anything about its house style.

## 5. Design

### 5.1 The command

`unluminate_core::document` gains one command and one small type:

```rust
pub enum IndentUnit { Tab, Space }

Command::Indent { unit: IndentUnit }
```

`IndentUnit` is the character the key names: `Tab` is `"\t"`, `Space` is `" "`. The command is
total — it answers for a selection and for a bare caret alike — because the command line asks it
about a document that may have neither:

- **With a selection**, the lines it indents are the lines the selection **touches**: from the line
  holding the selection's start to the line holding the byte before its end. A selection that ends
  exactly at a line break touches the line above the break, not the line below it, because it holds
  no byte of the line below.
- **With no selection**, it indents the line the caret is on, and the caret moves past the
  character put in front of it, so the caret stays after the indentation of the line it was on.

Each line is indented by inserting the unit's character at the line's start, back to front so an
earlier line's offset is not moved by a later line's edit. The insertion goes through the same three
lines `insert` and `replace_many` use — the text, the character formatting and the marked passages
moving together, with the folds and the breakpoints shifted in the same two places — because a place
that knew the bytes had moved and forgot one of the three is the fault this codebase has already
paid for. The inserted character takes the style of the position it is put in, read the way
`insert` reads it, and the pending formatting is not applied, because an indent is not typing.

**The selection follows the text it covered.** Each end of the selection moves past the indents put
at or before it, no further. That is the same shift the marks and the folds get, applied to the two
numbers a selection is, and it has the consequence the feature needs: pressing the key again indents
the same lines again, and the highlight stays over the words rather than jumping.

The edit is `EditKind::Other`, one `push_undo`, one `mark_changed`: whatever the selection spans,
one key press is one snapshot and one undo step, for the reason `ReplaceMany` states. Nothing about
the paragraph list moves, because no line break is inserted or removed.

### 5.2 The keys

`handle_input` is the one place the window's keys become commands, and it changes in two places:

- The bare `Tab` arm — already guarded so that `Ctrl+Tab` is `Next Tab` and not a typed tab — now
  asks whether there is a selection. With one, it applies `Indent { unit: Tab }`; without one it
  inserts the tab it always did. `Shift+Tab` takes the same arm, because the guard only excludes
  the command key and control, and outdent is not here (section 2).
- The `Text` arm asks whether the event is a single space and there is a selection and no command
  key or control is held. With all three, it applies `Indent { unit: Space }`; anything else is
  inserted as it always was. The modifier check is read off the frame's input state, because a `Text`
  event carries no modifiers of its own, and it is what keeps `Ctrl+Space` — the completion's key —
  from indenting on a platform where it should arrive as a space.

A space pressed with a selection while the completion popup is open indents like anywhere else:
the popup is a list over the document, and the document is what the key is for.

### 5.3 The cursor

`components::dock::handle` stops asking for a cursor at all. The two `set_cursor_icon` calls go,
and the header is the normal arrow from hover through the drop. The feedback a carried panel needs
is already drawn: the four faint bands and the strong rectangle the panel would land in, which are
`app::dock::regions` run over the layout as it would be after the drop, and they are on screen for
the whole of the drag. A cursor is a second channel saying the same thing, and the one the ticket
does not want.

The tab strips are left alone: they show the grabbing hand only while a tab is actually being
carried, which is the drag feedback rather than the hover the ticket names, and a tab in the air
with an arrow under it would be the drag the person cannot see.

### 5.4 The command line

`unluminate-cli editor indent` is the agent's half, and it is a row in the catalogue like every other
command with no menu entry:

```
unluminate-cli editor indent [--space]
```

It indents the lines the selection touches, or the caret's line when nothing is selected, with a tab
by default and a space with `--space` — the two keys, one command each. It goes through the same
`Command` the keys do, in `UnluminateApp::run_cli`'s one place a command turns into a change, so an
indent done by an agent and the same thing done by hand are the same thing. The MCP tool, the
usage line and the documentation section come from the catalogue, and the tests that fail while a
command is not offered as a tool or has no section in `commands.md` cover it without new code.

## 6. Tests

- **`unluminate-core`, no window.** `Indent` over a multi-line selection puts a character at the start of
  each touched line and nowhere else, and leaves the bytes between the line starts exactly as they
  were; a selection ending on a line break indents the line above it and not the one below; a bare
  caret indents its own line and moves past the character; the selection's ends each move by the
  indents at or before them, so a second press indents the same lines; one press is one undo step
  and undo restores the text and the selection; the tab unit and the space unit differ only in the
  character; and `a_layout_that_changed_means_the_text_revision_moved` applies the new command
  beside the others, so a later edit that changes the layout without moving the text revision fails
  the day it is written.
- **The screenshot tests.** A selection of two lines and a `Tab` press leaves the two lines indented
  and the selection over them; the same with `Space`; and a pointer over a panel's header leaves the
  window's cursor icon the default rather than the grab, read from the frame the way the heartbeat
  test reads the repaint delay, because a cursor is not in a picture.
- **The command line.** `editor indent` and `editor indent --space` through the window's own
  dispatcher, asserting the text they leave behind, beside the tests the other editor commands have.

## 7. Verification

The screenshot tests are the baseline, and the images are opened, because that is how a person
confirms the indents are where they should be. On top of that the change is released with
`tools/release.ps1` and the installed window is driven with `unluminate-cli` — a selection made, `Tab`
pressed, the text read back, and a screenshot taken of the result — because the ticket asks for the
thing to be seen, and the copy on the desktop is the build that answers "is this the one with the
fix in it".
