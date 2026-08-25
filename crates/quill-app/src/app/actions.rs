//! What the menus hold, and one list of everything a menu can ask Quill to do.
//!
//! There are two menu bars. On macOS the menus belong in the bar along the top of the screen, which is
//! built with `muda` in `services::native_menu`. Everywhere else they are drawn inside the window by
//! `components::menu_bar`. Both are built from [`menus`], so there is one place that decides what
//! `File` holds and what its shortcuts are, and the two bars cannot drift apart.
//!
//! [`Action`] is what a menu produces. Nothing here does anything: `QuillApp::run_action` is the one
//! place an action turns into a change, so the menu, the keyboard and a test all go down the same path.

use std::path::PathBuf;

use crate::app::ViewMode;

/// Everything a menu, or a keyboard shortcut belonging to a menu, can ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Another Quill window, on its own project.
    NewWindow,
    /// Choose a folder and open it in a window of its own, leaving this one as it is.
    ///
    /// A project is a window, the way IntelliJ has it, so there is one entry rather than the two there
    /// used to be. `Open Folder` used to replace the project in this window and `Open Folder in New
    /// Window` sat under it doing what this does now; `task-1658` asks for the second behaviour and
    /// two entries that do the same thing are worse than one.
    OpenFolder,
    /// Choose a file and open it in the editor.
    OpenFile,
    /// Open a project that has been open before, in a window of its own.
    OpenRecent(PathBuf),
    /// Forget the recent projects.
    ForgetRecent,
    Save,
    SaveAs,
    CloseWindow,
    /// Open the Settings modal.
    Settings,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Show the raw source, the source and preview together, or the preview.
    SetViewMode(ViewMode),
    /// Show or hide the file explorer.
    ToggleExplorer,
    /// Show or hide the column of line numbers down the left of the editing area.
    ToggleLineNumbers,
    /// Set the editor's text one size larger, or one smaller, walking the sizes the Settings window
    /// offers. It is the same setting the dialog holds, so it reaches every open file and is still
    /// there next time Quill starts.
    ChangeFontSize { larger: bool },
    /// Put the editor's text back to the size a new Quill has.
    ResetFontSize,
    /// Show or hide the terminal along the bottom.
    ToggleTerminal,
    /// Close the file tab that is showing.
    CloseTab,
    /// Show the next file tab, wrapping round at the end.
    NextTab,
    /// Show the previous file tab.
    PreviousTab,
    /// Another terminal tab.
    NewTerminalTab,
    /// Close the terminal tab that is showing.
    CloseTerminalTab,
    /// Make an empty file in this folder. The name is asked for first.
    NewFile(PathBuf),
    /// Hold this path to be moved when something is pasted.
    CutPath(PathBuf),
    /// Hold this path to be copied when something is pasted.
    CopyPath(PathBuf),
    /// Put this path's text on the system clipboard.
    CopyPathReference(PathBuf),
    /// Put whatever was cut or copied into this folder.
    PasteInto(PathBuf),
    /// Rename this file or folder. The new name is asked for first.
    RenamePath(PathBuf),
    /// Show this path in the platform's file manager.
    RevealPath(PathBuf),
    /// Read this folder again, and this file again if it is open.
    ReloadPath(PathBuf),
    /// Anything on the Git menu.
    Git(GitAction),
    /// Quill's own about box, which is a line in the status bar rather than a window.
    About,
    Quit,
}

impl Action {
    /// True when a text box that has the keyboard does this for itself.
    ///
    /// Undo, redo and select all mean the box being typed in and not the document while one of the
    /// window's text boxes has the keyboard, which is what every editor does and what egui's own
    /// text box already implements. Without this, control and Z in the explorer's filter box cleared
    /// the box and undid an edit in the file behind it with the one press.
    ///
    /// Three, and no more. Cut, copy and paste are already marked as not coming from the keyboard,
    /// because the platform delivers them as clipboard events, so they never reach the keyboard
    /// watcher at all. Everything else on every menu keeps working while a box has the keyboard:
    /// control and S in a search box saves the file, as it does in every other editor.
    pub fn belongs_to_a_focused_text_box(&self) -> bool {
        matches!(self, Action::Undo | Action::Redo | Action::SelectAll)
    }
}

/// Everything the Git menu can ask for.
///
/// A group of its own rather than twenty more variants of [`Action`], because they all go to one
/// place and because a menu with twenty entries reads better as a list than as twenty lines in an
/// enum shared with `Save`.
///
/// The ones that take a path take `None` to mean the file that is open, so one entry serves both the
/// Git menu and a right click on a row in the explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAction {
    /// Open the commit panel, or shut it when it is already open. The rail's git button and the menu
    /// entry are the same thing, so pressing the button twice puts the panel away again.
    Commit,
    /// Stage a path.
    Add(Option<PathBuf>),
    /// Show what changed in a file against what git has.
    ShowDiff(Option<PathBuf>),
    /// The same, against a revision that is asked for.
    CompareWithRevision(Option<PathBuf>),
    /// The commits that touched a file, or the whole repository when there is no path.
    ShowHistory(Option<PathBuf>),
    /// The commit that is checked out.
    ShowCurrentRevision,
    /// Throw away the changes to a path. Asked about first, because there is no undo for it.
    Rollback(Option<PathBuf>),
    /// Annotate the open file with git blame, or stop annotating it.
    Annotate,
    Push,
    Pull,
    Fetch,
    Merge,
    Rebase,
    /// Finish, or abandon, a merge or a rebase that stopped on a conflict.
    Continue,
    Abort,
    Branches,
    NewBranch,
    NewTag,
    ResetHead,
    Stash,
    Unstash,
    Remotes,
    Clone,
    /// Open the file where a path is ignored without changing what is committed.
    Exclude,
    /// Read the repository again.
    Refresh,
}

/// A keyboard shortcut, held as egui's own key and modifiers so that matching a key press is a
/// comparison rather than a translation.
///
/// `command` is the Apple key on macOS and the control key on Windows, which is what egui reports for a
/// shortcut on either platform, and what a person means by the modifier a menu shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: egui::Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    /// The control key itself, as distinct from `command`. Only the terminal uses it, for Control and
    /// backtick, because that is the shortcut every editor with a terminal uses.
    pub ctrl: bool,
}

impl Shortcut {
    /// The command key and a key, which is nearly every shortcut Quill has.
    pub const fn command(key: egui::Key) -> Self {
        Self { key, command: true, shift: false, alt: false, ctrl: false }
    }

    pub const fn command_shift(key: egui::Key) -> Self {
        Self { key, command: true, shift: true, alt: false, ctrl: false }
    }

    pub const fn control(key: egui::Key) -> Self {
        Self { key, command: false, shift: false, alt: false, ctrl: true }
    }

    pub const fn control_shift(key: egui::Key) -> Self {
        Self { key, command: false, shift: true, alt: false, ctrl: true }
    }

    /// True when this key press is this shortcut, and not a longer one that happens to include it.
    ///
    /// The two platforms have to be told apart, because they do not agree about what the control key
    /// is. On macOS the Apple key and the control key are two keys, and egui reports the Apple key as
    /// both `command` and `mac_cmd` while leaving `ctrl` alone, so the two can be compared
    /// separately. Everywhere else they are **one key**: egui sets `command` equal to `ctrl`, so a
    /// press of control satisfies a shortcut asking for either, and a shortcut asking for both would
    /// be asking for the same key twice.
    ///
    /// Comparing all four fields on both platforms, which this used to do, meant that on Windows
    /// every shortcut in the bar was unreachable: `Ctrl+S` arrives with `command` and `ctrl` both
    /// set, so `Save`, which asks for `command` and not `ctrl`, never matched. The tests did not
    /// catch it because they built a modifier set with `command` set and `ctrl` clear, which is a
    /// combination Windows never produces. They now build the set the platform really sends.
    pub fn matches(&self, key: egui::Key, modifiers: &egui::Modifiers) -> bool {
        // `+` and `=` are one key on nearly every layout, and `+` is the shifted one, so a shortcut
        // asking for plus accepts either and does not care whether shift is held: what a person
        // means by "control and plus" is that key, however their keyboard happens to label it. The
        // numeric keypad sends plus with no shift at all, which is the same key press again. Every
        // other shortcut still compares shift exactly, which is what keeps `Cmd+S` and `Cmd+Shift+S`
        // apart.
        if self.key == egui::Key::Plus {
            if !matches!(key, egui::Key::Plus | egui::Key::Equals) || modifiers.alt != self.alt {
                return false;
            }
        } else if self.key != key || modifiers.shift != self.shift || modifiers.alt != self.alt {
            return false;
        }
        if cfg!(target_os = "macos") {
            modifiers.command == self.command && (modifiers.ctrl && !modifiers.mac_cmd) == self.ctrl
        } else {
            // Either flag counts as the control key being held. The platform sets both, and
            // `egui::Modifiers::COMMAND` — which is what a test presses — sets only `command`.
            let control = modifiers.ctrl || modifiers.command;
            control == (self.command || self.ctrl)
        }
    }

    /// What a menu shows to the right of the entry, spelled out in words.
    ///
    /// Words rather than the Apple symbols: the command symbol at U+2318 is in egui's fonts but the shift
    /// symbol at U+21E7 is not, and it came out as an empty box. Mixing one symbol with one word reads
    /// worse than spelling both, and words work on either platform.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" });
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push(if cfg!(target_os = "macos") { "Option" } else { "Alt" });
        }
        if self.shift {
            parts.push("Shift");
        }
        let key = key_name(self.key);
        parts.push(key);
        parts.join("+")
    }
}

/// The name of a key as a menu spells it.
pub fn key_name(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Comma => ",",
        egui::Key::Backtick => "`",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        other => other.name(),
    }
}

/// One row of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Something to do, with the shortcut a menu shows next to it.
    Item {
        name: String,
        action: Action,
        shortcut: Option<Shortcut>,
        /// False when it cannot be done just now, such as undo with nothing to undo. The row is drawn
        /// dimmed and takes no clicks.
        enabled: bool,
        /// True when it is switched on, such as the view mode that is showing.
        checked: bool,
        /// False when the shortcut belongs to something else and this menu must not watch the keyboard
        /// for it.
        ///
        /// Cut, copy and paste are the only ones. Inside the window they are delivered by egui as
        /// clipboard events rather than as key presses, because that is how the platform hands over the
        /// clipboard, so watching for the key press as well would do the work twice.
        keyboard: bool,
    },
    /// A line between two groups.
    Separator,
    /// A menu inside a menu, which is what Recent Projects is.
    Submenu { name: String, entries: Vec<Entry> },
}

impl Entry {
    fn item(name: &str, action: Action) -> Self {
        Entry::Item {
            name: name.to_owned(),
            action,
            shortcut: None,
            enabled: true,
            checked: false,
            keyboard: true,
        }
    }

    fn with_shortcut(name: &str, action: Action, shortcut: Shortcut) -> Self {
        match Entry::item(name, action) {
            Entry::Item { name, action, enabled, checked, keyboard, .. } => Entry::Item {
                name,
                action,
                shortcut: Some(shortcut),
                enabled,
                checked,
                keyboard,
            },
            other => other,
        }
    }

    fn enabled(self, yes: bool) -> Self {
        match self {
            Entry::Item { name, action, shortcut, checked, keyboard, .. } => {
                Entry::Item { name, action, shortcut, enabled: yes, checked, keyboard }
            }
            other => other,
        }
    }

    fn checked(self, yes: bool) -> Self {
        match self {
            Entry::Item { name, action, shortcut, enabled, keyboard, .. } => {
                Entry::Item { name, action, shortcut, enabled, checked: yes, keyboard }
            }
            other => other,
        }
    }

    /// Mark an entry whose shortcut is delivered another way, so the keyboard watcher leaves it alone.
    fn not_from_the_keyboard(self) -> Self {
        match self {
            Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                Entry::Item { name, action, shortcut, enabled, checked, keyboard: false }
            }
            other => other,
        }
    }
}

/// One menu in the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// What the menus need to know about the window to say what can be done and what is switched on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub recent: Vec<PathBuf>,
    pub view_mode: ViewMode,
    /// True when the open file has a preview worth switching to, which is what dims the three view
    /// mode entries for a source file. The toolbar asks the same question of the same function, so
    /// the menu and the buttons cannot disagree about whether there is a preview.
    pub can_preview: bool,
    pub explorer_visible: bool,
    pub line_numbers: bool,
    pub terminal_visible: bool,
    pub terminal_tabs: usize,
    /// True when the folder that is open is in a git repository. With none, every git entry is
    /// dimmed rather than absent, so the menu does not change shape depending on where you are.
    pub in_repository: bool,
    /// True when a file is open that git could say something about.
    pub has_file: bool,
    /// True when the open file has been annotated with blame.
    pub annotated: bool,
    /// Set while a merge or a rebase has stopped on a conflict.
    pub unfinished: Option<&'static str>,
    /// How many files are open, which is what decides whether the tab entries can be used.
    pub open_files: usize,
}

/// The whole menu bar: `Quill`, `File`, `Edit` and `View`, in that order.
///
/// `Quill` comes first because that is where the application's own entries belong, and because macOS puts
/// the application menu first whatever it is called. Inside the window it is drawn first for the same
/// reason, so the bar reads `Quill  File  Edit  View` on both platforms.
pub fn menus(state: &MenuState) -> Vec<Menu> {
    vec![quill_menu(), file_menu(state), edit_menu(state), view_menu(state), git_menu(state)]
}

/// The Git menu, which holds what the reference capture in `tasks/quill-ide-tdd.md` holds.
///
/// The same entries, aimed at one path, are the `Git` submenu on a row in the explorer, built by
/// [`git_submenu`], so the two cannot drift apart.
///
/// `Continue` and `Abort` are there only while a merge or a rebase has stopped on a conflict. An
/// editor that hides a half-finished merge is an editor you cannot finish one in.
pub fn git_menu(state: &MenuState) -> Menu {
    let here = state.in_repository;
    let file = here && state.has_file;
    let mut entries = vec![
        Entry::with_shortcut("Commit...", Action::Git(GitAction::Commit), Shortcut::command(egui::Key::K))
            .enabled(here),
        Entry::with_shortcut(
            "Add",
            Action::Git(GitAction::Add(None)),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::A) },
        )
        .enabled(file),
        Entry::item("Exclude from Version Control", Action::Git(GitAction::Exclude)).enabled(here),
        Entry::Separator,
        Entry::with_shortcut("Show Diff", Action::Git(GitAction::ShowDiff(None)), Shortcut::command(egui::Key::D))
            .enabled(file),
        Entry::item("Compare with Revision...", Action::Git(GitAction::CompareWithRevision(None)))
            .enabled(file),
        Entry::item("Show History", Action::Git(GitAction::ShowHistory(None))).enabled(here),
        Entry::item("Show Current Revision", Action::Git(GitAction::ShowCurrentRevision)).enabled(here),
        Entry::item(
            if state.annotated { "Close Annotations" } else { "Annotate with Git Blame" },
            Action::Git(GitAction::Annotate),
        )
        .enabled(file)
        .checked(state.annotated),
        Entry::Separator,
        Entry::with_shortcut(
            "Rollback...",
            Action::Git(GitAction::Rollback(None)),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::Z) },
        )
        .enabled(file),
        Entry::Separator,
    ];
    if state.unfinished.is_some() {
        entries.push(Entry::item("Continue", Action::Git(GitAction::Continue)));
        entries.push(Entry::item("Abort", Action::Git(GitAction::Abort)));
        entries.push(Entry::Separator);
    }
    entries.extend([
        Entry::with_shortcut("Push...", Action::Git(GitAction::Push), Shortcut::command_shift(egui::Key::K))
            .enabled(here),
        Entry::item("Pull...", Action::Git(GitAction::Pull)).enabled(here),
        Entry::item("Fetch", Action::Git(GitAction::Fetch)).enabled(here),
        Entry::Separator,
        Entry::item("Merge...", Action::Git(GitAction::Merge)).enabled(here),
        Entry::item("Rebase...", Action::Git(GitAction::Rebase)).enabled(here),
        Entry::Separator,
        Entry::with_shortcut(
            "Branches...",
            Action::Git(GitAction::Branches),
            Shortcut::control_shift(egui::Key::Backtick),
        )
        .enabled(here),
        Entry::with_shortcut(
            "New Branch...",
            Action::Git(GitAction::NewBranch),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::N) },
        )
        .enabled(here),
        Entry::item("New Tag...", Action::Git(GitAction::NewTag)).enabled(here),
        Entry::item("Reset HEAD...", Action::Git(GitAction::ResetHead)).enabled(here),
        Entry::Separator,
        Entry::item("Stash Changes...", Action::Git(GitAction::Stash)).enabled(here),
        Entry::item("Unstash Changes...", Action::Git(GitAction::Unstash)).enabled(here),
        Entry::Separator,
        Entry::item("Manage Remotes...", Action::Git(GitAction::Remotes)).enabled(here),
        Entry::item("Clone...", Action::Git(GitAction::Clone)),
    ]);
    Menu { name: "Git".to_owned(), entries }
}

/// The `Git` submenu on a row in the explorer: the same entries, aimed at that row.
pub fn git_submenu(state: &MenuState, path: &std::path::Path) -> Vec<Entry> {
    let here = state.in_repository;
    vec![
        Entry::item("Add", Action::Git(GitAction::Add(Some(path.to_path_buf())))).enabled(here),
        Entry::item("Show Diff", Action::Git(GitAction::ShowDiff(Some(path.to_path_buf())))).enabled(here),
        Entry::item(
            "Compare with Revision...",
            Action::Git(GitAction::CompareWithRevision(Some(path.to_path_buf()))),
        )
        .enabled(here),
        Entry::item("Show History", Action::Git(GitAction::ShowHistory(Some(path.to_path_buf()))))
            .enabled(here),
        Entry::item("Rollback...", Action::Git(GitAction::Rollback(Some(path.to_path_buf()))))
            .enabled(here),
        Entry::Separator,
        Entry::item("Commit...", Action::Git(GitAction::Commit)).enabled(here),
    ]
}

fn quill_menu() -> Menu {
    Menu {
        name: "Quill".to_owned(),
        // Settings is in two menus on purpose. `tasks/improvements.md` names both `Quill -> Settings` and
        // `Edit -> Settings`, and both are where a person looks: the application menu is where macOS keeps a
        // program's own settings, and the Edit menu is where Windows does. The shortcut is on the Edit entry
        // only, because two menu items claiming one key equivalent is a fault on macOS.
        entries: vec![
            Entry::item("About Quill", Action::About),
            Entry::Separator,
            Entry::item("Settings", Action::Settings),
            Entry::Separator,
            Entry::with_shortcut("Quit Quill", Action::Quit, Shortcut::command(egui::Key::Q)),
        ],
    }
}

fn file_menu(state: &MenuState) -> Menu {
    let mut entries = vec![
        Entry::with_shortcut(
            "New Window",
            Action::NewWindow,
            Shortcut::command_shift(egui::Key::N),
        ),
        Entry::Separator,
        Entry::with_shortcut("Open File", Action::OpenFile, Shortcut::command(egui::Key::O)),
        // A project of its own in a window of its own, which is how a second project is opened without
        // giving up the one that is open.
        Entry::with_shortcut(
            "Open Folder",
            Action::OpenFolder,
            Shortcut::command_shift(egui::Key::O),
        ),
        Entry::Submenu { name: "Recent Projects".to_owned(), entries: recent_entries(state) },
        Entry::Separator,
        Entry::with_shortcut("Save", Action::Save, Shortcut::command(egui::Key::S)),
        Entry::with_shortcut("Save As", Action::SaveAs, Shortcut::command_shift(egui::Key::S)),
        Entry::Separator,
        Entry::with_shortcut(
            "Close Window",
            Action::CloseWindow,
            Shortcut::command(egui::Key::W),
        ),
    ];
    entries.retain(|entry| !matches!(entry, Entry::Submenu { entries, .. } if entries.is_empty()));
    Menu { name: "File".to_owned(), entries }
}

/// The recent projects, newest first, with a way to forget them.
///
/// A project the window already has open is still listed, dimmed, because a list that changes length
/// depending on where you are is harder to use than one that does not.
fn recent_entries(state: &MenuState) -> Vec<Entry> {
    if state.recent.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<Entry> = state
        .recent
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            Entry::item(&name, Action::OpenRecent(path.clone()))
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::item("Forget Recent Projects", Action::ForgetRecent));
    entries
}

fn edit_menu(state: &MenuState) -> Menu {
    Menu {
        name: "Edit".to_owned(),
        entries: vec![
            Entry::with_shortcut("Undo", Action::Undo, Shortcut::command(egui::Key::Z))
                .enabled(state.can_undo),
            Entry::with_shortcut("Redo", Action::Redo, Shortcut::command_shift(egui::Key::Z))
                .enabled(state.can_redo),
            Entry::Separator,
            Entry::with_shortcut("Cut", Action::Cut, Shortcut::command(egui::Key::X))
                .enabled(state.has_selection)
                .not_from_the_keyboard(),
            Entry::with_shortcut("Copy", Action::Copy, Shortcut::command(egui::Key::C))
                .enabled(state.has_selection)
                .not_from_the_keyboard(),
            Entry::with_shortcut("Paste", Action::Paste, Shortcut::command(egui::Key::V))
                .not_from_the_keyboard(),
            Entry::with_shortcut("Select All", Action::SelectAll, Shortcut::command(egui::Key::A)),
            Entry::Separator,
            Entry::with_shortcut("Settings", Action::Settings, Shortcut::command(egui::Key::Comma)),
        ],
    }
}

fn view_menu(state: &MenuState) -> Menu {
    let explorer = if state.explorer_visible { "Hide Explorer" } else { "Show Explorer" };
    Menu {
        name: "View".to_owned(),
        entries: vec![
            Entry::with_shortcut(
                "Raw Markdown",
                Action::SetViewMode(ViewMode::Raw),
                Shortcut::command(egui::Key::Num1),
            )
            .checked(state.view_mode == ViewMode::Raw)
            .enabled(state.can_preview),
            Entry::with_shortcut(
                "Side by Side",
                Action::SetViewMode(ViewMode::SideBySide),
                Shortcut::command(egui::Key::Num2),
            )
            .checked(state.view_mode == ViewMode::SideBySide)
            .enabled(state.can_preview),
            Entry::with_shortcut(
                "Markdown Preview",
                Action::SetViewMode(ViewMode::Preview),
                Shortcut::command(egui::Key::Num3),
            )
            .checked(state.view_mode == ViewMode::Preview)
            .enabled(state.can_preview),
            Entry::Separator,
            Entry::with_shortcut(
                explorer,
                Action::ToggleExplorer,
                Shortcut::command(egui::Key::Num0),
            ),
            Entry::item(
                if state.line_numbers { "Hide Line Numbers" } else { "Show Line Numbers" },
                Action::ToggleLineNumbers,
            ),
            Entry::Separator,
            // The editor's font size, on the keyboard as it is in every other editor. They are menu
            // entries rather than keys watched for in the editing area for the reason the whole menu
            // exists: on macOS a shortcut on a menu item is a key equivalent and AppKit hands the
            // press to the menu before the window sees it, so a key read in `editor_view` would work
            // on Windows and be dead on macOS. There is no shortcut on `Reset Font Size`, because
            // the obvious one is command and zero and `Show Explorer` already has it.
            Entry::with_shortcut(
                "Increase Font Size",
                Action::ChangeFontSize { larger: true },
                Shortcut::command(egui::Key::Plus),
            ),
            Entry::with_shortcut(
                "Decrease Font Size",
                Action::ChangeFontSize { larger: false },
                Shortcut::command(egui::Key::Minus),
            ),
            Entry::item("Reset Font Size", Action::ResetFontSize),
            Entry::Separator,
            // Ctrl+F4 rather than the Apple key and W, which `Close Window` already claims. Two menu
            // items claiming one key equivalent is a fault on macOS, and there is a test for it.
            Entry::with_shortcut("Close Tab", Action::CloseTab, Shortcut::command(egui::Key::F4))
                .enabled(state.open_files > 1),
            Entry::with_shortcut("Next Tab", Action::NextTab, Shortcut::control(egui::Key::Tab))
                .enabled(state.open_files > 1),
            Entry::with_shortcut(
                "Previous Tab",
                Action::PreviousTab,
                Shortcut::control_shift(egui::Key::Tab),
            )
            .enabled(state.open_files > 1),
            Entry::Separator,
            Entry::with_shortcut(
                "Terminal",
                Action::ToggleTerminal,
                Shortcut::control(egui::Key::Backtick),
            )
            .checked(state.terminal_visible),
            Entry::item("New Terminal Tab", Action::NewTerminalTab),
            Entry::item("Close Terminal Tab", Action::CloseTerminalTab)
                .enabled(state.terminal_tabs > 0),
        ],
    }
}

/// What the explorer's right click menu holds, for the row that was clicked.
///
/// `directory` says whether the row is a folder, because a new file goes *in* a folder and *beside*
/// a file, and there is nothing to reload from disk about a file that is not open.
///
/// There is no `Delete`. IntelliJ's menu has one and `task-1649` does not ask for one, and a
/// destructive entry nobody asked for, one row under `Rename...`, is worth leaving out until it is
/// wanted. Cut and Paste already move a file out of the way.
pub fn explorer_menu(path: &std::path::Path, directory: bool, can_paste: bool) -> Vec<Entry> {
    let folder = if directory {
        path.to_path_buf()
    } else {
        path.parent().map(std::path::Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
    };
    vec![
        Entry::Submenu {
            name: "New".to_owned(),
            entries: vec![Entry::item("File", Action::NewFile(folder.clone()))],
        },
        Entry::Separator,
        Entry::with_shortcut("Cut", Action::CutPath(path.to_path_buf()), Shortcut::command(egui::Key::X))
            .not_from_the_keyboard(),
        Entry::with_shortcut("Copy", Action::CopyPath(path.to_path_buf()), Shortcut::command(egui::Key::C))
            .not_from_the_keyboard(),
        Entry::item("Copy Path", Action::CopyPathReference(path.to_path_buf())),
        Entry::with_shortcut("Paste", Action::PasteInto(folder), Shortcut::command(egui::Key::V))
            .enabled(can_paste)
            .not_from_the_keyboard(),
        Entry::Separator,
        Entry::item("Rename...", Action::RenamePath(path.to_path_buf())),
        Entry::Separator,
        Entry::item(crate::services::launcher::file_manager_name(), Action::RevealPath(path.to_path_buf())),
        Entry::item("Reload from Disk", Action::ReloadPath(path.to_path_buf())),
    ]
}

/// The explorer's menu with the `Git` submenu on the end, which is what is actually shown.
///
/// Split from [`explorer_menu`] so the entries that have nothing to do with git can be tested
/// without a repository behind them.
pub fn explorer_menu_with_git(
    state: &MenuState,
    path: &std::path::Path,
    directory: bool,
    can_paste: bool,
) -> Vec<Entry> {
    let mut entries = explorer_menu(path, directory, can_paste);
    entries.push(Entry::Separator);
    entries.push(Entry::Submenu { name: "Git".to_owned(), entries: git_submenu(state, path) });
    entries
}

/// What the gutter's own right click menu holds.
///
/// Built here rather than in the component for the same reason the bar's menus are: an entry is an
/// [`Action`] with one arm in `run_action`, so the gutter's `Show Line Numbers` and the View menu's
/// are the same thing rather than two things that agree today.
pub fn gutter_menu(state: &MenuState) -> Vec<Entry> {
    vec![
        Entry::item(
            if state.annotated { "Close Annotations" } else { "Annotate with Git Blame" },
            Action::Git(GitAction::Annotate),
        )
        .enabled(state.in_repository && state.has_file)
        .checked(state.annotated),
        Entry::Separator,
        Entry::item(
            if state.line_numbers { "Hide Line Numbers" } else { "Show Line Numbers" },
            Action::ToggleLineNumbers,
        ),
    ]
}

/// The action a key press asks for, if any menu entry claims it.
///
/// Entries marked [`Entry::not_from_the_keyboard`] are skipped, because something else delivers those.
pub fn action_for_key(
    state: &MenuState,
    key: egui::Key,
    modifiers: &egui::Modifiers,
) -> Option<Action> {
    fn search(entries: &[Entry], key: egui::Key, modifiers: &egui::Modifiers) -> Option<Action> {
        for entry in entries {
            match entry {
                Entry::Item { action, shortcut: Some(shortcut), enabled, keyboard: true, .. }
                    if *enabled && shortcut.matches(key, modifiers) =>
                {
                    return Some(action.clone());
                }
                Entry::Submenu { entries, .. } => {
                    if let Some(found) = search(entries, key, modifiers) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    menus(state).iter().find_map(|menu| search(&menu.entries, key, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, .. } => Some(name.clone()),
                Entry::Submenu { name, .. } => Some(name.clone()),
                Entry::Separator => None,
            })
            .collect()
    }

    fn find(menus: &[Menu], menu: &str) -> Menu {
        menus
            .iter()
            .find(|found| found.name == menu)
            .cloned()
            .unwrap_or_else(|| panic!("there should be a {menu} menu"))
    }

    #[test]
    fn quill_comes_first_and_then_file_edit_view_and_git() {
        let bar = menus(&MenuState::default());
        let order: Vec<&str> = bar.iter().map(|menu| menu.name.as_str()).collect();
        assert_eq!(order, vec!["Quill", "File", "Edit", "View", "Git"]);
    }

    #[test]
    fn every_git_entry_is_dimmed_outside_a_repository_rather_than_missing() {
        // A menu that changes shape depending on where you are is harder to use than one that does
        // not, so the entries stay and are dimmed. `Clone...` is the exception: it is how you come
        // to have a repository at all.
        let outside = git_menu(&MenuState::default());
        let usable: Vec<String> = outside
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, enabled: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(usable, vec!["Clone...".to_owned()]);
        assert!(
            names(&outside.entries).len() > 15,
            "the entries are still there, just dimmed: {:?}",
            names(&outside.entries)
        );
    }

    #[test]
    fn continue_and_abort_are_there_only_while_something_is_half_finished() {
        let settled = MenuState { in_repository: true, ..MenuState::default() };
        assert!(!names(&git_menu(&settled).entries).contains(&"Abort".to_owned()));
        let stuck = MenuState { unfinished: Some("Merging"), ..settled.clone() };
        let entries = names(&git_menu(&stuck).entries);
        assert!(entries.contains(&"Continue".to_owned()));
        assert!(entries.contains(&"Abort".to_owned()));
    }

    #[test]
    fn the_git_entries_on_a_row_in_the_explorer_are_aimed_at_that_row() {
        let state = MenuState { in_repository: true, ..MenuState::default() };
        let path = PathBuf::from("/project/notes.md");
        let entries = explorer_menu_with_git(&state, &path, false, false);
        let git = entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Submenu { name, entries } if name == "Git" => Some(entries.clone()),
                _ => None,
            })
            .expect("a Git submenu");
        let aimed = git.iter().any(|entry| {
            matches!(entry, Entry::Item { action: Action::Git(GitAction::ShowDiff(Some(at))), .. } if *at == path)
        });
        assert!(aimed, "Show Diff on a row is about that row's file");
    }

    #[test]
    fn the_gutter_menu_offers_blame_and_the_line_numbers() {
        let state = MenuState {
            in_repository: true,
            has_file: true,
            line_numbers: true,
            ..MenuState::default()
        };
        assert_eq!(
            names(&gutter_menu(&state)),
            vec!["Annotate with Git Blame", "Hide Line Numbers"]
        );
        // Once it is annotated the same entry closes the annotations, so there is one entry rather
        // than two that have to be kept in step.
        let annotated = MenuState { annotated: true, ..state };
        assert_eq!(names(&gutter_menu(&annotated))[0], "Close Annotations");
    }

    #[test]
    fn settings_can_be_reached_from_the_application_menu_and_from_the_edit_menu() {
        let bar = menus(&MenuState::default());
        assert!(names(&find(&bar, "Quill").entries).contains(&"Settings".to_owned()));
        assert!(names(&find(&bar, "Edit").entries).contains(&"Settings".to_owned()));
        // Only one of them carries the shortcut, because two menu items claiming one key equivalent is a
        // fault on macOS.
        let mut with_shortcut = 0;
        for menu in menus(&MenuState::default()) {
            for entry in &menu.entries {
                if let Entry::Item { name, shortcut: Some(_), .. } = entry {
                    if name == "Settings" {
                        with_shortcut += 1;
                    }
                }
            }
        }
        assert_eq!(with_shortcut, 1);
    }

    #[test]
    fn the_file_menu_holds_the_things_it_used_to_and_the_new_window() {
        let bar = menus(&MenuState::default());
        let file = names(&find(&bar, "File").entries);
        for expected in ["New Window", "Open File", "Open Folder", "Save", "Save As"] {
            assert!(file.contains(&expected.to_owned()), "File should hold {expected}, it has {file:?}");
        }
    }

    #[test]
    fn opening_a_folder_is_one_entry_and_it_opens_a_window_of_its_own() {
        // There used to be two, and they differed only in which window the project landed in.
        // `task-1658` asks for a project to be a window, so there is one entry.
        let file = names(&find(&menus(&MenuState::default()), "File").entries);
        let opens: Vec<&String> =
            file.iter().filter(|name| name.starts_with("Open Folder")).collect();
        assert_eq!(opens, vec![&"Open Folder".to_owned()]);
    }

    #[test]
    fn recent_projects_is_left_out_until_there_is_something_in_it() {
        let bar = menus(&MenuState::default());
        assert!(
            !names(&find(&bar, "File").entries).contains(&"Recent Projects".to_owned()),
            "an empty Recent Projects is not shown at all"
        );

        let state = MenuState { recent: vec![PathBuf::from("/tmp/one")], ..MenuState::default() };
        let bar = menus(&state);
        let file = find(&bar, "File");
        let submenu = file
            .entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Submenu { name, entries } if name == "Recent Projects" => Some(entries.clone()),
                _ => None,
            })
            .expect("Recent Projects should be there once a project has been opened");
        assert_eq!(names(&submenu), vec!["one", "Forget Recent Projects"]);
    }

    #[test]
    fn undo_is_dimmed_until_there_is_something_to_undo() {
        let bar = menus(&MenuState::default());
        let edit = find(&bar, "Edit");
        let undo = edit
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Undo"))
            .expect("Edit should hold Undo");
        assert!(matches!(undo, Entry::Item { enabled: false, .. }));

        let state = MenuState { can_undo: true, ..MenuState::default() };
        let bar = menus(&state);
        let edit = find(&bar, "Edit");
        let undo = edit
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Undo"))
            .expect("Edit should hold Undo");
        assert!(matches!(undo, Entry::Item { enabled: true, .. }));
    }

    #[test]
    fn the_view_menu_marks_the_mode_that_is_showing() {
        let state = MenuState { view_mode: ViewMode::Preview, ..MenuState::default() };
        let view = find(&menus(&state), "View");
        let checked: Vec<String> = view
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, checked: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, vec!["Markdown Preview"]);
    }

    /// The modifier set the platform really sends when the command key is held: on macOS the Apple
    /// key, which egui reports as `command` and `mac_cmd`; everywhere else the control key, which it
    /// reports as `command` and `ctrl` both.
    fn pressing_command() -> egui::Modifiers {
        if cfg!(target_os = "macos") {
            egui::Modifiers { command: true, mac_cmd: true, ..Default::default() }
        } else {
            egui::Modifiers { command: true, ctrl: true, ..Default::default() }
        }
    }

    /// The control key itself, which on macOS is not the command key and everywhere else is.
    fn pressing_control() -> egui::Modifiers {
        if cfg!(target_os = "macos") {
            egui::Modifiers { ctrl: true, ..Default::default() }
        } else {
            egui::Modifiers { command: true, ctrl: true, ..Default::default() }
        }
    }

    #[test]
    fn eguis_own_command_modifier_is_matched_as_well_as_the_platforms() {
        // A test presses `egui::Modifiers::COMMAND`, which sets `command` and leaves `ctrl` clear —
        // a combination no platform sends. Both have to work, or the shortcuts pass their tests and
        // do nothing in the real window, which is exactly what happened before this was fixed.
        let state = MenuState::default();
        assert_eq!(
            action_for_key(&state, egui::Key::S, &egui::Modifiers::COMMAND),
            Some(Action::Save)
        );
    }

    #[test]
    fn a_shortcut_the_platform_really_sends_is_matched() {
        // This is the case that used to fail on Windows: `Ctrl+S` arrives with `command` and `ctrl`
        // both set, and `Save` asks for the command key and not the control key.
        let state = MenuState::default();
        assert_eq!(action_for_key(&state, egui::Key::S, &pressing_command()), Some(Action::Save));
        assert_eq!(
            action_for_key(&state, egui::Key::Backtick, &pressing_control()),
            Some(Action::ToggleTerminal),
            "control and backtick opens the terminal on both platforms"
        );
    }

    #[test]
    fn the_font_size_can_be_changed_from_the_keyboard_however_plus_is_typed() {
        // `+` and `=` are one key, and `+` is the shifted one, so all three of these are a person
        // pressing "control and plus": the unshifted key, the shifted key, and the numeric keypad.
        let state = MenuState::default();
        let larger = Some(Action::ChangeFontSize { larger: true });
        assert_eq!(action_for_key(&state, egui::Key::Equals, &pressing_command()), larger);
        assert_eq!(action_for_key(&state, egui::Key::Plus, &pressing_command()), larger);
        let shifted = egui::Modifiers { shift: true, ..pressing_command() };
        assert_eq!(action_for_key(&state, egui::Key::Plus, &shifted), larger);
        assert_eq!(
            action_for_key(&state, egui::Key::Minus, &pressing_command()),
            Some(Action::ChangeFontSize { larger: false })
        );
    }

    #[test]
    fn the_plus_rule_does_not_loosen_any_other_shortcut() {
        // Only plus accepts an unasked-for shift. `Cmd+S` and `Cmd+Shift+S` are two different
        // entries and have to stay that way.
        let state = MenuState::default();
        let shifted = egui::Modifiers { shift: true, ..pressing_command() };
        assert_eq!(action_for_key(&state, egui::Key::S, &shifted), Some(Action::SaveAs));
        assert_eq!(action_for_key(&state, egui::Key::S, &pressing_command()), Some(Action::Save));
    }

    #[test]
    fn resetting_the_font_size_is_on_the_view_menu_without_a_shortcut() {
        // Command and zero is the obvious one and `Show Explorer` already claims it. Two entries on
        // one key equivalent is a fault on macOS, and there is a test above that would catch it.
        let view = find(&menus(&MenuState::default()), "View");
        let reset = view
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Reset Font Size"))
            .expect("Reset Font Size is on the View menu");
        assert!(matches!(reset, Entry::Item { shortcut: None, action: Action::ResetFontSize, .. }));
    }

    #[test]
    fn only_undo_redo_and_select_all_belong_to_a_focused_text_box() {
        // `task-1656`. While one of the window's text boxes has the keyboard these three mean that
        // box, so the keyboard watcher lets them go. The rest of the menu is untouched, which is the
        // half that stops the guard from being too broad.
        for action in [Action::Undo, Action::Redo, Action::SelectAll] {
            assert!(action.belongs_to_a_focused_text_box(), "{action:?} belongs to the box");
        }
        for action in [
            Action::Save,
            Action::SaveAs,
            Action::ToggleExplorer,
            Action::ToggleTerminal,
            Action::Settings,
            Action::SetViewMode(ViewMode::Preview),
        ] {
            assert!(
                !action.belongs_to_a_focused_text_box(),
                "{action:?} keeps working while a box has the keyboard"
            );
        }
    }

    #[test]
    fn a_shortcut_is_spelled_out_in_words() {
        assert_eq!(Shortcut::command(egui::Key::S).label(), if cfg!(target_os = "macos") { "Cmd+S" } else { "Ctrl+S" });
        assert_eq!(
            Shortcut::command_shift(egui::Key::O).label(),
            if cfg!(target_os = "macos") { "Cmd+Shift+O" } else { "Ctrl+Shift+O" }
        );
        assert_eq!(Shortcut::command(egui::Key::Comma).label().ends_with(','), true);
        assert_eq!(Shortcut::control(egui::Key::Backtick).label(), "Ctrl+`");
    }

    #[test]
    fn a_key_press_finds_the_action_whose_shortcut_it_is() {
        let state = MenuState { can_undo: true, can_redo: true, ..MenuState::default() };
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::S, &command), Some(Action::Save));
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), Some(Action::Undo));
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::Z, &with_shift), Some(Action::Redo));
        assert_eq!(action_for_key(&state, egui::Key::S, &with_shift), Some(Action::SaveAs));
    }

    #[test]
    fn a_shortcut_with_more_modifiers_is_not_mistaken_for_one_with_fewer() {
        let state = MenuState::default();
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::N, &command), None, "Cmd+N is not New Window");
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::N, &with_shift), Some(Action::NewWindow));
    }

    #[test]
    fn undo_that_cannot_be_done_is_not_taken_from_the_keyboard_either() {
        let state = MenuState::default();
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), None);
    }

    #[test]
    fn the_clipboard_shortcuts_are_left_to_the_platform() {
        // Cut, copy and paste reach the window as egui clipboard events. If they were watched for here as
        // well, one press would do the work twice.
        let state = MenuState { has_selection: true, ..MenuState::default() };
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::C, &command), None);
        assert_eq!(action_for_key(&state, egui::Key::V, &command), None);
        assert_eq!(action_for_key(&state, egui::Key::X, &command), None);
        // They are still in the menu, with their shortcuts shown.
        let edit = find(&menus(&state), "Edit");
        assert!(names(&edit.entries).contains(&"Paste".to_owned()));
    }

    #[test]
    fn every_shortcut_in_the_bar_is_claimed_by_one_entry_only() {
        let state = MenuState {
            can_undo: true,
            can_redo: true,
            has_selection: true,
            recent: vec![PathBuf::from("/tmp/one")],
            terminal_tabs: 1,
            ..MenuState::default()
        };
        let mut seen: Vec<(Shortcut, String)> = Vec::new();
        fn walk(entries: &[Entry], seen: &mut Vec<(Shortcut, String)>) {
            for entry in entries {
                match entry {
                    Entry::Item { name, shortcut: Some(shortcut), .. } => {
                        if let Some((_, other)) = seen.iter().find(|(seen, _)| seen == shortcut) {
                            panic!("{} and {other} both claim {}", name, shortcut.label());
                        }
                        seen.push((*shortcut, name.clone()));
                    }
                    Entry::Submenu { entries, .. } => walk(entries, seen),
                    _ => {}
                }
            }
        }
        for menu in menus(&state) {
            walk(&menu.entries, &mut seen);
        }
        assert!(seen.len() > 10, "there should be a shortcut on most entries, found {}", seen.len());
    }
}
