# Component baselines

One capture for each piece of the window, so that a new component can be compared against what the
ones beside it already look like. `task-1649` asks for exactly this: screenshots in the design folder
that are referenced when building a new component.

These are **intent**. `crates/quill-app/tests/snapshots` is a different thing and must not be
confused with it: those are accepted test output, and a change that alters the rendering fails
against them. Change one of these when the design changes; accept one of those when the drawing does.

Each was captured from the real window through `crates/quill-app/tests/screenshots.rs`, on Windows,
at 1180 x 740. They were all retaken for `task-1658`, which moved the text options into the title
bar, added the rail down the far left and gave the window its own resize grips: every one of them
shows a different window from the one it showed before.

| Image | What it is the baseline for |
|---|---|
| `gutter_line_numbers.png` | The line numbers, and how a wrapped paragraph is numbered once. |
| `gutter_blame.png` | The blame column: the tint from oldest to newest, the date and the first name. |
| `gutter_menu.png` | The menu a right click on the gutter opens. |
| `file_tabs.png` | The tab strip: the accent underline on the tab that is showing, the plugin's icon, the amber dot for unsaved changes. |
| `explorer_menu.png` | The explorer's right click menu, including its `Git` submenu. |
| `new_file_prompt.png` | The text prompt. Rename, New Branch, New Tag, Stash and Clone are all this shape. |
| `git_menu.png` | The Git menu, and how a long menu scrolls rather than running off the bottom. |
| `git_commit_panel.png` | The commit panel: the changes tree, the branch chip, `Unversioned Files`, the counts, the message box. |
| `git_branches.png` | A git dialog: a list, and the footer's buttons. Push, Pull, Merge, Rebase, Reset and Remotes are the same shape. |
| `git_history.png` | The history: the short hash, the refs, the subject, the author and the date. The one capture here that is not offscreen output, because a commit made during a test has a hash and a date that are new every run; it is the window from `documentation/images/13-git-history.jpg`, so it is 1800 x 1160 rather than 1180 x 740. |
| `syntax_typescript.png` | A file coloured by its plugin, in Dracula. |
| `plugins_page.png` | `Settings -> Plugins`: the tabs, the list, and a plugin's own page. |
| `settings_appearance.png` | The Settings window, which is the shape every modal in Quill is built from. |
| `terminal.png` | The terminal tile and its tabs. |
| `text_options.png` | The flyout behind the title bar's `F` button: the four named rows, the rule between the character half and the paragraph half, and the three line spacings as buttons. |
| `code_no_toolbar.png` | A source file, which has no text tools at all. The right hand end of the title bar is simply empty, and nothing below it moves. |
| `activity_bar.png` | The rail of pane buttons down the far left: the pill under a pane that is open, and the terminal's button at the bottom. |
| `picture.png` | A picture in a tab, scaled to fit the editing area, with its size and scale in the status bar. |

`design/style-guide.md` says what these are made of.
