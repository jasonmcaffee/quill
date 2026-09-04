# task-1682 — Renaming a terminal tab, rearranging the strip, and answering a modal from the keyboard

Three asks that look unrelated and are not. All three are about a control that could only be reached
with the pointer: a terminal tab could only be named by the program inside it, it could only be put
where it opened, and a dialog could only be answered by clicking the button at its bottom right.

> I should be able to right click a terminal tab, see a menu to rename, and be able to rename the tab.
>
> I should also be able to drag and drop to rearrange the terminal tabs.
>
> Also, for our modal dialogs, I should be able to press Enter to create/confirm/etc. Right now I
> have to manually click a button.

## 1. What a terminal tab is called

### 1.1 What it was

`Session::name` answered with the title the program set, falling back to the program's own name when
the title said no more than where the program was started from — `task-1670`'s answer to `cmd.exe`
putting `C:\Windows\system32\cmd.exe` on a Windows tab. `Tabs::names` then put a number after a name
that had been used before, so two shells were `powershell.exe` and `powershell.exe 2`.

Neither is a name a person chose, and the second is the thing that makes three shells impossible to
tell apart: which of them is running the build and which is running `claude` is a fact about what
somebody is doing, and no program is going to say it.

### 1.2 What was weighed

**Write the name into `title`.** One field instead of two, and it is what the tab already reads. It
is wrong for one reason and the reason is fatal: `claude` sets a title on every prompt, and so does
`git`, and so does anything using an OSC 0. A rename written into the title would last until the
program next spoke, which is a rename that appears to work and then quietly undoes itself.

**A name on the tab rather than on the session.** `unluminous-terminal`'s `Tabs` is a list of `Session`s
and nothing else — its own module comment says "a tab is a session and nothing else: there is no
state a tab carries that the session does not". A parallel `Vec<Option<String>>` would be a second
thing to keep in step with every `open`, `close` and `move_tab`, and the first one to forget would be
a name that followed the wrong tab after a close.

**What was chosen: a third field on `Session`.** `given` is empty until somebody renames the tab, and
`name()` answers with it before it looks at either of the other two. It rides along with the session
through every list operation for free, and the invariant is one sentence: *a name a person typed beats
a name a program set, and nothing a program does takes it away.*

An **empty** name puts the tab back to being named after its program, so there is one way to undo a
rename rather than a second command meaning "forget the name I gave". The dialog cannot ask for it —
its button needs something in the field — but `unluminous-cli terminal rename --tab 0` can, and that is
what a test asserts.

### 1.3 A name a person typed is never numbered

`Tabs::names` numbers repeats so that two tabs running the same program can be told apart. A name
somebody typed is left exactly as they typed it: the number exists to tell two tabs apart, and a
person who has called two tabs the same thing has already said what they want them called.

Working on this turned up a fault in the numbering itself, which is worth recording because it is the
reason renaming was asked for. The count was `names.iter().filter(|seen| seen.ends_with(&name))`,
compared against the names already worked out — and `powershell.exe 2` does not end with
`powershell.exe`, so the **third** shell was a second `powershell.exe 2`. Three tabs, two of them
with the same name. It counts against the name the session gives now, through a map, and
`the_third_tab_running_the_same_thing_is_the_third_and_not_a_second_second` pins it.

## 2. The right click menu

`components::terminal_panel` reports `PanelOutcome::menu` — which tab, and where the pointer was —
and the window opens `components::context_menu` on it, exactly as the explorer, the gutter and a file
tab's strip already do. The menu is `UnluminousApp::terminal_menu`, held in the window's own state rather
than in egui's memory for the reason the other three are: a screenshot test cannot press the right
mouse button, and a menu that can only be reached by pressing it cannot be looked at.

**Every entry is about the terminal tab that is showing**, which is `actions::tab_menu`'s rule
restated. That is what makes them ordinary parameterless `Action`s the View menu, the keyboard and
`unluminous-cli action run` can all ask for without inventing a way to name a tab — so the strip **shows**
the tab a right click landed on before the menu opens. `actions::terminal_tab_menu` takes no
`MenuState` at all, because the menu can only be opened by right clicking a tab and there is
therefore always one to rename and one to close.

`Rename Terminal Tab...` is also on the `View` menu beside `New Terminal Tab` and `Close Terminal
Tab`. Not for symmetry: `unluminous-cli action list` is built by walking the real menus, and a context
menu is not one of them, so an entry that lived only in the right click menu would be an entry the
command line could not find.

The prompt itself is the one `components::prompt_dialog` already draws, with a new
`Purpose::RenameTerminalTab(index)`. It carries the **number** rather than meaning "the tab that is
showing", because a prompt is answered on some later frame and which tab is showing can change in
between.

## 3. Dragging a tab along the strip

The mechanism is `task-1673`'s, and two of its three parts are used unchanged: `file_tabs::Strip`,
which is where a strip drew itself and each of its tabs, and `file_tabs::insertion_mark`, the two
point accent line down the gap. A tab goes **after every tab whose middle the pointer has passed**,
which is what makes a rearrangement follow the pointer rather than jump when it crosses an edge.

The third part is deliberately **not** used. A file tab's drag is settled by the window, because a
tab picked up in one pane is very often dropped on another and no one strip can see them all. There
is one strip of terminal tabs, so the strip a tab is picked up from is the strip it is dropped on,
and `tab_strip` settles it itself — which is also where `close` and `show` are already settled.

`Tabs::move_tab(index, position)` is the move, and `position` counts the tabs **as they are on the
screen now**, including the one being carried, because that is what a person dragging it is looking
at. Taking it out first shifts everything after it up by one, so a move to a place further along has
one subtracted from it inside that function rather than at every call. `OpenFiles::drag_tab` already
says the same sentence about file tabs, and `unluminous-cli terminal move` and the drag both go through
it, so a rearrangement made from a script and one made with the pointer are the same rearrangement.

The tab that was moved is the one showing afterwards, which is what dragging something somewhere
means and is what a file tab already does.

## 4. Enter answers a modal

### 4.1 One place decides it

`components::modal::footer` is the bar across the bottom of every modal built from the shared
furniture: the About box, `Go to File`, `Find in Files`, the references modal, the commit panel and
all eight git dialogs. Its own comment already said that the **last** button is the one that does the
thing and is filled in the accent colour, so there was already a primary button; it simply had no
key.

It has one now, and because it is decided there, a dialog written later gets it without asking. The
two dialogs that draw their own footer — `prompt_dialog`, which the text prompt and the confirmation
share, and the Settings window — ask the same function rather than answering a second way.

**A footer whose last button is dimmed is a modal there is nothing to confirm**, so the key press is
left alone rather than doing nothing loudly.

### 4.2 The one modal that owns Enter

The commit message is a `TextEdit::multiline`, where Enter is a new line and has to stay one.
`modal::Confirm` has two values for that reason: `Enter` for every modal in Unluminous but one, and
`CommandEnter` for the commit panel, which is IntelliJ's own chord for the same dialog. Both of that
modal's tabs use it — one modal, one key — because Enter alone in the `Stashes` tab would pop a stash
for somebody who pressed it meaning nothing.

### 4.3 Why the comparison is `command_only` and not an equality test

`InputState::consume_key` matches by `Modifiers::matches_logically`, which only asks whether the
modifiers the *pattern* names are held; a pattern of `NONE` therefore matches `Command+Enter` as
well, and the commit dialog would have committed on both. This is the same trap `task-1678`'s
completion popup fell into, where a pattern of `NONE` swallowed `Shift+Enter`.

The first attempt at avoiding it compared the held modifiers against `Modifiers::COMMAND` for
equality. That passes a test and fails in the window: **on Windows `Ctrl+Enter` arrives with both
`ctrl` and `command` set**, so the equality never held. `Modifiers::is_none` and
`Modifiers::command_only` are the two questions that are true of what a keyboard really sends.

The press is **taken out of the frame's input** once it has been read, so a modal confirmed from the
keyboard cannot also be read as an ordinary Enter by anything drawn after it.

The three list dialogs — `Go to File`, `Find in Files` and the references modal — already take Enter
for themselves *before* their footer is drawn, where it means "open the row that is chosen". Nothing
had to change there, and nothing should: in those the button at the bottom right is the same thing
said twice.

### 4.4 A modal takes the keyboard

This is the part that had to come with it. `text_box_has_the_keyboard` is how the editing area and
the terminal already stand aside for a box that is being typed into, and it is `ctx.text_edit_focused()`
— so it says nothing at all about a **confirmation**, an about box or most of the git dialogs, none of
which has a field in it. Behind those, the editing area, the terminal and the explorer went on
reading the frame's keys.

Giving Enter a meaning in every modal turned that from a latent oddity into a real fault: `Enter` in
the delete confirmation would have deleted the file **and** inserted a new line in the file behind it
**and** opened the row the explorer's cursor was on.

`a_modal_has_the_keyboard` is the other half of the same question, asked of egui's own modal layer
rather than of a list of Unluminous's dialogs, so a modal added later is covered without being added
anywhere. It is the layer as it stood at the **end of the last frame**, which is the honest answer at
the point those three read the keyboard: before anything is drawn, and so before this frame's modal
has said it is there.

## 5. The command line

`unluminous-cli terminal rename [--tab <index>] <name>` and `unluminous-cli terminal move [--tab <index>]
<position>`, both through the same two functions the window uses. The name takes the rest of the line
so it needs no quotes, which is what `terminal send` already does, and `--tab` therefore comes before
it.

`unluminous-cli action run rename-terminal-tab` opens the prompt, because the action is on a real menu.

## 6. What is deliberately not here

**A shortcut for the rename.** IntelliJ has none for renaming a terminal tab either, and every chord
worth having in a terminal already reaches the program inside it — which is `task-1670`'s whole
subject.

**Remembering a name across restarts.** `services::project_state` restores "the same number of
shells in the project's folder" and says why: what a program was doing when the window closed cannot
be brought back. A name is about what the shell is *for*, so restoring the name onto a fresh shell
would be a label that might be a lie. It is worth doing the day a terminal tab remembers anything
else about itself, and not before.

**Dragging a terminal tab out of the strip.** There is one strip and nowhere else to put it.

**A three-answer dialog for Enter.** VS Code has a three-valued setting for whether Enter accepts a
completion, and `task-1678` records why Unluminous does not need one. The same reasoning holds here: the
one modal where Enter means something else says so with a different chord, rather than every modal
growing a preference.

## 7. Tests

- `unluminous-terminal`: a typed name beats a program's title and survives one being set; an empty name
  puts it back; a typed name is never numbered; the third tab of a kind is the third; a tab dragged
  along the strip lands where the pointer left it; a tab picked up and put back moves nothing.
- `unluminous-app` screenshots, which feed real events through the real window:
  `a_terminal_tab_is_renamed_from_its_own_menu` (the menu, the prompt, and the title a program sets
  afterwards), `a_terminal_tab_is_dragged_along_the_strip` (with a picture taken mid-drag, showing
  the tab in the air and the mark where it would land),
  `dragging_a_terminal_tab_and_the_command_line_are_the_same_rearrangement`,
  `enter_presses_the_button_that_does_the_thing`,
  `enter_answers_a_question_that_has_no_field_in_it_and_reaches_nothing_behind_it`,
  `a_modal_takes_the_keyboard_from_the_editing_area_and_the_explorer`, and
  `enter_in_the_commit_message_is_a_new_line_and_the_command_key_commits`, which asserts both halves
  against a real repository.
