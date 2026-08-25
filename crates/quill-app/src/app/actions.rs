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
    /// Choose a folder and show it in this window's explorer.
    OpenFolder,
    /// Choose a folder and open it in a window of its own, leaving this one as it is.
    OpenFolderInNewWindow,
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
    /// Show or hide the terminal along the bottom.
    ToggleTerminal,
    /// Another terminal tab.
    NewTerminalTab,
    /// Close the terminal tab that is showing.
    CloseTerminalTab,
    /// Quill's own about box, which is a line in the status bar rather than a window.
    About,
    Quit,
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

    /// True when this key press is this shortcut, and not a longer one that happens to include it.
    pub fn matches(&self, key: egui::Key, modifiers: &egui::Modifiers) -> bool {
        self.key == key
            && modifiers.command == self.command
            && modifiers.shift == self.shift
            && modifiers.alt == self.alt
            // On macOS egui reports the Apple key as both `command` and `mac_cmd`, and leaves `ctrl`
            // alone, so the control key can be told apart from it.
            && (modifiers.ctrl && !modifiers.mac_cmd) == self.ctrl
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
#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub recent: Vec<PathBuf>,
    pub view_mode: ViewMode,
    pub explorer_visible: bool,
    pub terminal_visible: bool,
    pub terminal_tabs: usize,
}

/// The whole menu bar: `Quill`, `File`, `Edit` and `View`, in that order.
///
/// `Quill` comes first because that is where the application's own entries belong, and because macOS puts
/// the application menu first whatever it is called. Inside the window it is drawn first for the same
/// reason, so the bar reads `Quill  File  Edit  View` on both platforms.
pub fn menus(state: &MenuState) -> Vec<Menu> {
    vec![quill_menu(), file_menu(state), edit_menu(state), view_menu(state)]
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
        Entry::with_shortcut(
            "Open Folder",
            Action::OpenFolder,
            Shortcut::command_shift(egui::Key::O),
        ),
        // A project of its own in a window of its own, which is how a second project is opened without
        // giving up the one that is open.
        Entry::with_shortcut(
            "Open Folder in New Window",
            Action::OpenFolderInNewWindow,
            Shortcut { alt: true, ..Shortcut::command(egui::Key::O) },
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
            .checked(state.view_mode == ViewMode::Raw),
            Entry::with_shortcut(
                "Side by Side",
                Action::SetViewMode(ViewMode::SideBySide),
                Shortcut::command(egui::Key::Num2),
            )
            .checked(state.view_mode == ViewMode::SideBySide),
            Entry::with_shortcut(
                "Markdown Preview",
                Action::SetViewMode(ViewMode::Preview),
                Shortcut::command(egui::Key::Num3),
            )
            .checked(state.view_mode == ViewMode::Preview),
            Entry::Separator,
            Entry::with_shortcut(
                explorer,
                Action::ToggleExplorer,
                Shortcut::command(egui::Key::Num0),
            ),
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
    fn quill_comes_first_and_then_file_edit_and_view() {
        let bar = menus(&MenuState::default());
        let order: Vec<&str> = bar.iter().map(|menu| menu.name.as_str()).collect();
        assert_eq!(order, vec!["Quill", "File", "Edit", "View"]);
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
        for expected in [
            "New Window",
            "Open File",
            "Open Folder",
            "Open Folder in New Window",
            "Save",
            "Save As",
        ] {
            assert!(file.contains(&expected.to_owned()), "File should hold {expected}, it has {file:?}");
        }
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
        let command = egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ..Default::default() };
        assert_eq!(action_for_key(&state, egui::Key::S, &command), Some(Action::Save));
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), Some(Action::Undo));
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::Z, &with_shift), Some(Action::Redo));
        assert_eq!(action_for_key(&state, egui::Key::S, &with_shift), Some(Action::SaveAs));
    }

    #[test]
    fn a_shortcut_with_more_modifiers_is_not_mistaken_for_one_with_fewer() {
        let state = MenuState::default();
        let command = egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ..Default::default() };
        assert_eq!(action_for_key(&state, egui::Key::N, &command), None, "Cmd+N is not New Window");
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::N, &with_shift), Some(Action::NewWindow));
    }

    #[test]
    fn undo_that_cannot_be_done_is_not_taken_from_the_keyboard_either() {
        let state = MenuState::default();
        let command = egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ..Default::default() };
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), None);
    }

    #[test]
    fn the_clipboard_shortcuts_are_left_to_the_platform() {
        // Cut, copy and paste reach the window as egui clipboard events. If they were watched for here as
        // well, one press would do the work twice.
        let state = MenuState { has_selection: true, ..MenuState::default() };
        let command = egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ..Default::default() };
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
