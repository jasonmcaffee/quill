# task-1670 — the terminal opened in the wrong folder, running the wrong shell

`task-1670` reported one thing and turned out to be four. This records what each was, how it was
found, what was changed and what was deliberately not changed.

The report, in full:

> When I open a terminal in Unluminate, it shows:
>
> ```text
> \\?\C:\jason\dev\unluminate'
> CMD.EXE was started with the above path as the current directory.
> UNC paths are not supported.  Defaulting to Windows directory.
> Microsoft Windows [Version 10.0.26100.6584]
> ```
>
> And has me at `C:\Windows`.
>
> Then I quit and opened again and now it shows `C:\Users\jason\AppData\Local\Programs\Unluminate>^\`
> and my commands like `claude-skip` don't work.

Four faults, and none of them is the one the message names. They are set out in the order they bite.

## 1. The working directory was a verbatim path

`std::fs::canonicalize` on Windows does not give back `C:\jason\dev\unluminate`. It gives back
`\\?\C:\jason\dev\unluminate` — a **verbatim** path: the form that lets a path exceed 260 characters and
that is handed to the file system with none of the parsing an ordinary path goes through.

Nothing inside Unluminate notices, because every Rust file call takes it happily. So it travelled.
`Store::remember_project` canonicalised, wrote the result into `recent.txt`, and from there it became
the explorer's root, the folder `.unluminate` is written beside, and the directory a shell is started in.
Every one of the nine lines in the recent projects file on the reporting machine was verbatim.

`cmd.exe` is the one program in the chain that refuses it. Two leading backslashes are the start of a
network share as far as it is concerned, it says so, and it starts in `C:\Windows` instead. That is
worse than an error: the terminal opens, works, and is quietly in the wrong folder.

**Fixed in two places, and the two are answering different questions.**

`unluminate_terminal::paths::plain` takes the prefix off. It lives beside the code that starts other
programs rather than in the window, because the window is not the only thing that hands a directory
over — a test, an example and `unluminate-cli terminal` all can, and a list of the places that have to
remember to strip it is a list whose next entry will be the one that forgot. `Session::spawn` calls
it, so no caller can get this wrong.

`Store::remember_project` calls it as well, so that a verbatim path is never written down in the first
place, and `Store::recent_projects` calls it while **reading**, so the list already on somebody's disk
is repaired rather than left for them to edit by hand.

`\\?\UNC\server\share` becomes `\\server\share`, which is the ordinary spelling of the same place.
`\\?\Volume{…}` is left exactly as it is: a volume with no drive letter has no shorter spelling, and
taking the prefix off it would name somewhere else.

The regression test is
`session::tests::a_shell_starts_in_the_folder_it_was_given_even_when_the_path_is_verbatim`. It
canonicalises a real folder, asserts that what came back really is verbatim — the test is worth
nothing on a system where it is not — starts a real shell in it, and asks the shell where it is
standing. Two things about how it asks are worth keeping.

It asks with `echo [%CD%]` rather than `cd`. The first draft used `cd` and looked for the folder's name
anywhere on the screen, and it **passed without the fix**, because when `cmd` refuses a folder it
prints the path it was given as part of the complaint. The path being on the screen proves nothing;
only the path in brackets is the shell saying it started there.

And it fails loudly rather than slowly: the wait loop asserts that `UNC paths are not supported` has
not appeared, so the fault comes back as the message that describes it rather than as a timeout.

## 2. The shell was `cmd.exe`, so nothing in a PowerShell profile existed

`default_shell()` read `COMSPEC` on Windows. `COMSPEC` names the interpreter that runs a **batch
file**, and on every Windows there has ever been it says `cmd.exe`. It is not an answer to "which
shell has this person chosen", because Windows has no such variable — but it looks like one, which is
why it was there.

So Unluminate opened a `cmd` while the reporting machine's own commands live in

```text
C:\Users\jason\Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1
```

as PowerShell functions. `claude-skip` cannot exist in `cmd`; it is
`'claude-skip' is not recognized`, in a terminal that looks like every other terminal on the machine.

**It is PowerShell now** — `pwsh.exe` when it is on the path, `powershell.exe` otherwise, and
`COMSPEC` only as a last resort for a Windows with neither. Choosing the newest installed is what
Windows Terminal does, and the two are not two spellings of one shell: they read **different**
profiles, `Documents\PowerShell` and `Documents\WindowsPowerShell`, so which one is started decides
which set of a person's own commands the terminal comes up holding.

Whether a program is on the path is answered by looking for the file in the folders `PATH` names,
rather than by running it, because this is asked once for every terminal opened and starting a program
to ask whether it exists would put a window on the screen on the one system where that matters.

### And it is a setting

`terminal.shell`, empty by default. `Edit -> Settings -> Tools -> Terminal -> Shell`, and
`unluminate-cli settings set terminal.shell cmd.exe` for a person who wants `cmd` back. Three things about
it were decided deliberately:

- **Empty means "what this machine says"**, and `Settings::shell()` is the one function that says so,
  rather than an `is_empty` test at each of the places that start a terminal.
- **Nothing is written to the settings file until it is chosen**, so the file does not name a shell on
  every machine it is copied to. A settings file is copied between machines; which shell exists is a
  fact about one of them.
- **The value is not checked against the machine**, the way a font family is. A shell may be a bare
  name to be found on the path, an absolute path, or something that will be installed in a minute. When
  it is wrong the tile says so in the operating system's own words, which is `Tabs::open`'s existing
  answer and a better message than one invented here.

`new_terminal_tab` is the one place the setting reaches a tab, so the screenshot test and
`examples/terminal_capture` set the setting rather than the tab's own copy — which means they exercise
the path the released binary takes.

## 3. Reopening Unluminate landed in the folder Unluminate is installed in

With no path on the command line, Unluminate showed the current directory. That is the honest answer when a
person typed `unluminate` in a terminal: they are standing in the folder they mean. It is not an answer at
all when Unluminate was started from the desktop, the Start menu or a file association, because the current
directory is then whatever the shortcut points at — which for Unluminate's own installer is the folder
`unluminate.exe` sits in. Hence `C:\Users\jason\AppData\Local\Programs\Unluminate>`, and hence that folder
appearing in the recent projects list, because opening it is what Unluminate did.

`unluminate_app::starting_folder` is the rule: **when the current directory is the folder the program itself
lives in, nobody chose it, and the project that was open last time is what was meant.** Otherwise the
current directory stands.

That is narrower than "always reopen the last project", and deliberately. `unluminate` typed in a folder has
to open *that* folder or the command line is lying about what it does. A last project that has since
been deleted falls back to the current directory rather than to nothing.

The recent projects are read in `main.rs`, before the window is built, rather than waiting for
`load_settings` — which folder to show has to be decided first. It is still the released binary reading
the person's own files, which is the rule `services::store` keeps.

## 4. The stray `^\` on the prompt

`^\` is how `cmd` echoes byte 0x1c. Enlarging the screenshot settled that it really was those two
characters and not part of a path.

0x1c is the control code made from `4`, which is standard — `Ctrl+4` sends it in xterm and in Windows
Terminal too. What is not standard is that Unluminate sent it **with shift held**. `Shift+4` is not `4`; it
is `$` on this layout and `"` on a British one, and which symbol it is depends on the keyboard, so
there is no control code to be had from it. `Ctrl+Shift+4` takes a screenshot on the reporting
machine, and the screenshot key left its own residue on the prompt in the picture it took.

A digit or a piece of punctuation held with shift now sends nothing. A **letter** is untouched: shift
does not change which letter it is, and `Ctrl+Shift+C` is `Ctrl+C` in every terminal there is.

### The larger fault behind it

Looking at that code found something worse. The key encoding asked `actions::key_name` for the
character a key stands for — and `key_name` is what a **menu** shows, so it spells the punctuation as
words: `Backslash`, `OpenBracket`, `Semicolon`, because `Ctrl+Backslash` reads better in a menu than
`Ctrl+\`. A word is not one character, so every one of those keys fell out of the encoder and was sent
as **nothing at all**.

Which means `Ctrl+]` did nothing in Unluminate's terminal — and `Ctrl+]` is how a person detaches from
`claude`, as the comment in the very profile function this ticket is about says. So did `Ctrl+\`, which
quits a program, and `Ctrl+Space`, which sends a null.

`components::terminal_panel::symbol` is the answer: the character a key stands for **as a terminal
reads it**, which is a different question from what a menu shows and now has a different function.
Two keys are named there rather than taken from egui — the minus key, because egui spells it with the
typographic minus sign at U+2212 rather than the hyphen a shell reads, and space, whose name is a word.

## What was deliberately not done

- **`unluminate-git` still canonicalises.** `Repository::discover` gives back a verbatim root on Windows,
  and `git` itself takes one, so nothing is broken by it. It was left alone rather than swept into a
  ticket about the terminal.
- **The explorer's root is not normalised.** With the store fixed it is plain in practice, and the
  terminal strips the prefix at the point of use whatever it is handed. A third place doing the same
  work would be a third place to keep in step.
- **`Ctrl+4` still sends 0x1c.** It is what every other terminal does, and the fault was the shift, not
  the digit.
- **`COMSPEC` was not simply swapped for a hard-coded `powershell.exe`.** A machine with `pwsh`
  installed keeps its profile somewhere else, so the choice has to be made by looking.

## What proves it

| What | Where |
|---|---|
| The prefix comes off, and only when it is safe to | `unluminate-terminal/src/paths.rs` — five unit tests |
| A real shell starts in a folder named the verbatim way | `session::tests::a_shell_starts_in_the_folder_it_was_given_even_when_the_path_is_verbatim` |
| The default on Windows is PowerShell | `session::tests::the_default_shell_is_powershell_rather_than_the_batch_interpreter` |
| Nothing verbatim is written down, and an old list is repaired | `store::tests::a_remembered_project_is_written_down_as_a_plain_path`, `…a_list_written_by_an_earlier_unluminate_is_read_as_plain_paths` |
| An empty shell setting means the machine's own | `settings::tests::no_shell_chosen_means_the_one_this_machine_says_the_person_has` |
| The shell typed in the dialog is what a tab runs | `screenshots::the_shell_typed_into_the_settings_is_what_a_new_terminal_runs` |
| The Shell section is on the page and looks right | `screenshots::the_terminal_page_holds_the_font_size_and_the_shell` |
| `Ctrl+]`, `Ctrl+\`, `Ctrl+Space` reach the program | `terminal_panel::tests::control_and_a_piece_of_punctuation_reaches_the_program` |
| A shifted digit sends nothing, a shifted letter still does | `terminal_panel::tests::control_and_a_shifted_digit_sends_nothing` |
| The desktop and the terminal start Unluminate in different folders | `tests::started_from_the_desktop_shows_the_project_that_was_open_last_time`, `…started_from_a_terminal_shows_the_folder_the_person_is_standing_in` |
