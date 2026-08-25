# Component baselines

One capture for each piece of the window, so that a new component can be compared against what the
ones beside it already look like. `task-1649` asks for exactly this: screenshots in the design folder
that are referenced when building a new component.

These are **intent**. `crates/quill-app/tests/snapshots` is a different thing and must not be
confused with it: those are accepted test output, and a change that alters the rendering fails
against them. Change one of these when the design changes; accept one of those when the drawing does.

Each was captured from the real window through `crates/quill-app/tests/screenshots.rs`, on Windows,
at 1180 x 740.

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
| `git_history.png` | The history: the short hash, the refs, the subject, the author and the date. |
| `syntax_typescript.png` | A file coloured by its plugin, in Dracula. |
| `plugins_page.png` | `Settings -> Plugins`: the tabs, the list, and a plugin's own page. |
| `settings_appearance.png` | The Settings window, which is the shape every modal in Quill is built from. |
| `terminal.png` | The terminal tile and its tabs. |

`design/style-guide.md` says what these are made of.
