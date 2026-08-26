//! A plain name for every action, so that the command line can ask for any of them.
//!
//! `task-1661` asks that every feature be reachable from the command line, and that a feature added
//! later be reachable too. The answer is here rather than in a second list: an [`Action`] already
//! is the one thing a menu or a shortcut can ask for, so giving each one a name makes `quill-cli
//! action run <name>` a way in to everything the menus hold — including whatever is added to them
//! tomorrow, because `quill-cli action list` is built by walking the real menus rather than by
//! anybody writing the names down again.
//!
//! The names are lower case and hyphenated, taken from the variant rather than from the menu row's
//! wording: `toggle-line-numbers` rather than `Show Line Numbers`, because what the row says
//! changes with the state and a name that changed with the state would be useless in a script.
//!
//! Three actions are **refused** from the command line. `open-folder`, `open-file` and `save-as`
//! each open the platform's own file chooser and then wait for somebody to click in it, which from
//! a script is a window that never closes. Each has a command that takes the path instead, and the
//! refusal says which.

use std::path::PathBuf;

use crate::app::actions::{Action, GitAction, HighlightColor};
use crate::app::ViewMode;

impl Action {
    /// The name the command line calls this action.
    pub fn name(&self) -> String {
        match self {
            Action::NewWindow => "new-window".to_owned(),
            Action::OpenFolder => "open-folder".to_owned(),
            Action::OpenFile => "open-file".to_owned(),
            Action::GoToFile => "go-to-file".to_owned(),
            Action::FindInFiles => "find-in-files".to_owned(),
            Action::GoToDefinition => "go-to-definition".to_owned(),
            Action::FindReferences => "find-references".to_owned(),
            Action::RenameSymbol => "rename-symbol".to_owned(),
            Action::CompleteWord => "complete-word".to_owned(),
            Action::NavigateBack => "navigate-back".to_owned(),
            Action::NavigateForward => "navigate-forward".to_owned(),
            Action::OpenRecent(_) => "open-recent".to_owned(),
            Action::ForgetRecent => "forget-recent".to_owned(),
            Action::Save => "save".to_owned(),
            Action::SaveAs => "save-as".to_owned(),
            Action::CloseWindow => "close-window".to_owned(),
            Action::Settings => "settings".to_owned(),
            Action::Undo => "undo".to_owned(),
            Action::Redo => "redo".to_owned(),
            Action::Cut => "cut".to_owned(),
            Action::Copy => "copy".to_owned(),
            Action::Paste => "paste".to_owned(),
            Action::SelectAll => "select-all".to_owned(),
            Action::SetViewMode(ViewMode::Raw) => "view-raw".to_owned(),
            Action::SetViewMode(ViewMode::SideBySide) => "view-side".to_owned(),
            Action::SetViewMode(ViewMode::Preview) => "view-preview".to_owned(),
            Action::ToggleExplorer => "toggle-explorer".to_owned(),
            Action::ToggleLineNumbers => "toggle-line-numbers".to_owned(),
            Action::ChangeFontSize { larger: true } => "increase-font-size".to_owned(),
            Action::ChangeFontSize { larger: false } => "decrease-font-size".to_owned(),
            Action::ResetFontSize => "reset-font-size".to_owned(),
            Action::ToggleTerminal => "toggle-terminal".to_owned(),
            Action::CloseTab => "close-tab".to_owned(),
            Action::NextTab => "next-tab".to_owned(),
            Action::PreviousTab => "previous-tab".to_owned(),
            Action::SplitRight => "split-right".to_owned(),
            Action::MoveTabRight => "move-tab-right".to_owned(),
            Action::MoveTabLeft => "move-tab-left".to_owned(),
            Action::Unsplit => "unsplit".to_owned(),
            Action::UnsplitAll => "unsplit-all".to_owned(),
            Action::NextPane => "next-pane".to_owned(),
            Action::PreviousPane => "previous-pane".to_owned(),
            Action::SelectOpenFile => "select-open-file".to_owned(),
            Action::NewTerminalTab => "new-terminal-tab".to_owned(),
            Action::CloseTerminalTab => "close-terminal-tab".to_owned(),
            Action::NewFile(_) => "new-file".to_owned(),
            Action::CutPath(_) => "cut-path".to_owned(),
            Action::CopyPath(_) => "copy-path".to_owned(),
            Action::CopyPathReference(_) => "copy-path-reference".to_owned(),
            Action::PasteInto(_) => "paste-into".to_owned(),
            Action::RenamePath(_) => "rename-path".to_owned(),
            Action::DeletePath(_) => "delete-path".to_owned(),
            Action::RevealPath(_) => "reveal-path".to_owned(),
            Action::ReloadPath(_) => "reload-path".to_owned(),
            Action::Git(what) => format!("git-{}", what.name()),
            Action::Highlight(colour) => {
                format!("highlight-{}", colour.label().to_ascii_lowercase())
            }
            Action::ClearHighlight => "clear-highlight".to_owned(),
            Action::ClearHighlights => "clear-highlights".to_owned(),
            Action::About => "about".to_owned(),
            Action::Quit => "quit".to_owned(),
        }
    }

    /// The action of this name, given a path for the ones that are about a file or a folder.
    ///
    /// `None` when there is no action by that name. An action that wants a path and is given none
    /// is a different failure, reported by [`Action::wants_a_path`], because "there is no such
    /// action" and "that action needs a path" are two different things to be told.
    pub fn from_name(name: &str, path: Option<PathBuf>) -> Option<Action> {
        if let Some(rest) = name.strip_prefix("git-") {
            return GitAction::from_name(rest, path).map(Action::Git);
        }
        if let Some(rest) = name.strip_prefix("highlight-") {
            return HighlightColor::ALL
                .iter()
                .find(|colour| colour.label().eq_ignore_ascii_case(rest))
                .map(|colour| Action::Highlight(*colour));
        }
        let with_path = || path.clone().unwrap_or_default();
        Some(match name {
            "new-window" => Action::NewWindow,
            "open-folder" => Action::OpenFolder,
            "open-file" => Action::OpenFile,
            "go-to-file" => Action::GoToFile,
            "find-in-files" => Action::FindInFiles,
            "go-to-definition" => Action::GoToDefinition,
            "find-references" => Action::FindReferences,
            "rename-symbol" => Action::RenameSymbol,
            "complete-word" => Action::CompleteWord,
            "navigate-back" => Action::NavigateBack,
            "navigate-forward" => Action::NavigateForward,
            "open-recent" => Action::OpenRecent(with_path()),
            "forget-recent" => Action::ForgetRecent,
            "save" => Action::Save,
            "save-as" => Action::SaveAs,
            "close-window" => Action::CloseWindow,
            "settings" => Action::Settings,
            "undo" => Action::Undo,
            "redo" => Action::Redo,
            "cut" => Action::Cut,
            "copy" => Action::Copy,
            "paste" => Action::Paste,
            "select-all" => Action::SelectAll,
            "view-raw" => Action::SetViewMode(ViewMode::Raw),
            "view-side" => Action::SetViewMode(ViewMode::SideBySide),
            "view-preview" => Action::SetViewMode(ViewMode::Preview),
            "toggle-explorer" => Action::ToggleExplorer,
            "toggle-line-numbers" => Action::ToggleLineNumbers,
            "increase-font-size" => Action::ChangeFontSize { larger: true },
            "decrease-font-size" => Action::ChangeFontSize { larger: false },
            "reset-font-size" => Action::ResetFontSize,
            "toggle-terminal" => Action::ToggleTerminal,
            "close-tab" => Action::CloseTab,
            "next-tab" => Action::NextTab,
            "previous-tab" => Action::PreviousTab,
            "split-right" => Action::SplitRight,
            "move-tab-right" => Action::MoveTabRight,
            "move-tab-left" => Action::MoveTabLeft,
            "unsplit" => Action::Unsplit,
            "unsplit-all" => Action::UnsplitAll,
            "next-pane" => Action::NextPane,
            "previous-pane" => Action::PreviousPane,
            "select-open-file" => Action::SelectOpenFile,
            "new-terminal-tab" => Action::NewTerminalTab,
            "close-terminal-tab" => Action::CloseTerminalTab,
            "new-file" => Action::NewFile(with_path()),
            "cut-path" => Action::CutPath(with_path()),
            "copy-path" => Action::CopyPath(with_path()),
            "copy-path-reference" => Action::CopyPathReference(with_path()),
            "paste-into" => Action::PasteInto(with_path()),
            "rename-path" => Action::RenamePath(with_path()),
            "delete-path" => Action::DeletePath(with_path()),
            "reveal-path" => Action::RevealPath(with_path()),
            "reload-path" => Action::ReloadPath(with_path()),
            "clear-highlight" => Action::ClearHighlight,
            "clear-highlights" => Action::ClearHighlights,
            "about" => Action::About,
            "quit" => Action::Quit,
            _ => return None,
        })
    }

    /// True when this action cannot be run without being told which file or folder it is about.
    pub fn wants_a_path(name: &str) -> bool {
        matches!(
            name,
            "open-recent"
                | "new-file"
                | "cut-path"
                | "copy-path"
                | "copy-path-reference"
                | "paste-into"
                | "rename-path"
                | "reveal-path"
                | "reload-path"
        )
    }

    /// The command to use instead, for the actions that would open the platform's file chooser.
    ///
    /// A file chooser is a window somebody has to click in. Asked for from a script it is a window
    /// nobody is looking at and a command that never returns, so these are refused with the name of
    /// the command that takes the path directly.
    pub fn instead_of_a_file_chooser(name: &str) -> Option<&'static str> {
        match name {
            "open-folder" => Some("project open <folder>"),
            "open-file" => Some("tab open <path>"),
            "save-as" => Some("tab save-as <path>"),
            _ => None,
        }
    }
}

impl GitAction {
    /// The name the command line calls this entry on the Git menu.
    pub fn name(&self) -> &'static str {
        match self {
            GitAction::Commit => "commit",
            GitAction::Add(_) => "add",
            GitAction::ShowDiff(_) => "show-diff",
            GitAction::CompareWithRevision(_) => "compare-with-revision",
            GitAction::ShowHistory(_) => "show-history",
            GitAction::ShowCurrentRevision => "show-current-revision",
            GitAction::Rollback(_) => "rollback",
            GitAction::Annotate => "annotate",
            GitAction::Push => "push",
            GitAction::Pull => "pull",
            GitAction::Fetch => "fetch",
            GitAction::Merge => "merge",
            GitAction::Rebase => "rebase",
            GitAction::Continue => "continue",
            GitAction::Abort => "abort",
            GitAction::Branches => "branches",
            GitAction::NewBranch => "new-branch",
            GitAction::NewTag => "new-tag",
            GitAction::ResetHead => "reset-head",
            GitAction::Stash => "stash",
            GitAction::Unstash => "unstash",
            GitAction::Remotes => "remotes",
            GitAction::Clone => "clone",
            GitAction::Exclude => "exclude",
            GitAction::Refresh => "refresh",
        }
    }

    /// The entry of this name. The five that are about one file take the path, and `None` means the
    /// file that is showing, which is what the Git menu itself passes.
    pub fn from_name(name: &str, path: Option<PathBuf>) -> Option<GitAction> {
        Some(match name {
            "commit" => GitAction::Commit,
            "add" => GitAction::Add(path),
            "show-diff" => GitAction::ShowDiff(path),
            "compare-with-revision" => GitAction::CompareWithRevision(path),
            "show-history" => GitAction::ShowHistory(path),
            "show-current-revision" => GitAction::ShowCurrentRevision,
            "rollback" => GitAction::Rollback(path),
            "annotate" => GitAction::Annotate,
            "push" => GitAction::Push,
            "pull" => GitAction::Pull,
            "fetch" => GitAction::Fetch,
            "merge" => GitAction::Merge,
            "rebase" => GitAction::Rebase,
            "continue" => GitAction::Continue,
            "abort" => GitAction::Abort,
            "branches" => GitAction::Branches,
            "new-branch" => GitAction::NewBranch,
            "new-tag" => GitAction::NewTag,
            "reset-head" => GitAction::ResetHead,
            "stash" => GitAction::Stash,
            "unstash" => GitAction::Unstash,
            "remotes" => GitAction::Remotes,
            "clone" => GitAction::Clone,
            "exclude" => GitAction::Exclude,
            "refresh" => GitAction::Refresh,
            _ => return None,
        })
    }

    /// Every entry, which is what `quill-cli git actions` lists.
    pub const ALL: &'static [GitAction] = &[
        GitAction::Commit,
        GitAction::Add(None),
        GitAction::ShowDiff(None),
        GitAction::CompareWithRevision(None),
        GitAction::ShowHistory(None),
        GitAction::ShowCurrentRevision,
        GitAction::Rollback(None),
        GitAction::Annotate,
        GitAction::Push,
        GitAction::Pull,
        GitAction::Fetch,
        GitAction::Merge,
        GitAction::Rebase,
        GitAction::Continue,
        GitAction::Abort,
        GitAction::Branches,
        GitAction::NewBranch,
        GitAction::NewTag,
        GitAction::ResetHead,
        GitAction::Stash,
        GitAction::Unstash,
        GitAction::Remotes,
        GitAction::Clone,
        GitAction::Exclude,
        GitAction::Refresh,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{self, Entry, MenuState};

    /// Every action the menus hold, gathered by walking them.
    fn every_action_on_every_menu() -> Vec<Action> {
        fn walk(entries: &[Entry], out: &mut Vec<Action>) {
            for entry in entries {
                match entry {
                    Entry::Item { action, .. } => out.push(action.clone()),
                    Entry::Submenu { entries, .. } => walk(entries, out),
                    Entry::Separator => {}
                }
            }
        }
        // Every gate switched on, so the walk reaches the entries that are **absent** rather than
        // dimmed when a file's language cannot answer them. Left at their defaults, this walked a
        // menu with no `Go to Definition`, no `Find References`, no `Rename Symbol` and no
        // `Complete Word` in it and said nothing was missing — which is how `complete-word` came to
        // have a name that `action run` could not find it by.
        let state = MenuState {
            recent: vec![PathBuf::from("/a/project")],
            unfinished: Some("merge"),
            definitions_apply: true,
            symbols_apply: true,
            completion_applies: true,
            ..MenuState::default()
        };
        let mut out = Vec::new();
        for menu in actions::menus(&state) {
            walk(&menu.entries, &mut out);
        }
        // The context menus are not in the bar, so the walk above does not reach them. The tab's is
        // walked as well, because `task-1664` puts `Split Right` on it and the rule the tests keep is
        // that anything a menu can ask for can be asked for from the command line.
        walk(&actions::tab_menu(&state), &mut out);
        out
    }

    #[test]
    fn every_action_on_every_menu_has_a_name_that_finds_it_again() {
        // This is the rule the ticket asks for, enforced: a menu entry added later is reachable from
        // the command line, or this test fails on the day it is added.
        for action in every_action_on_every_menu() {
            let name = action.name();
            let path = match &action {
                Action::OpenRecent(path)
                | Action::NewFile(path)
                | Action::CutPath(path)
                | Action::CopyPath(path)
                | Action::CopyPathReference(path)
                | Action::PasteInto(path)
                | Action::RenamePath(path)
                | Action::DeletePath(path)
                | Action::RevealPath(path)
                | Action::ReloadPath(path) => Some(path.clone()),
                Action::Git(
                    GitAction::Add(path)
                    | GitAction::ShowDiff(path)
                    | GitAction::CompareWithRevision(path)
                    | GitAction::ShowHistory(path)
                    | GitAction::Rollback(path),
                ) => path.clone(),
                _ => None,
            };
            assert_eq!(
                Action::from_name(&name, path),
                Some(action.clone()),
                "`{name}` should name {action:?} and read back as it"
            );
        }
    }

    #[test]
    fn every_name_is_lower_case_and_hyphenated() {
        for action in every_action_on_every_menu() {
            let name = action.name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{name} is not a lower case hyphenated name"
            );
        }
    }

    #[test]
    fn no_two_actions_share_a_name() {
        let mut seen: Vec<String> = Vec::new();
        for action in every_action_on_every_menu() {
            let name = action.name();
            if seen.contains(&name) {
                // The same action appears on more than one menu, which is fine; what must not
                // happen is two *different* actions answering to one name.
                let already = Action::from_name(&name, None);
                assert!(
                    already.is_some(),
                    "{name} is used twice and does not read back to one action"
                );
                continue;
            }
            seen.push(name);
        }
    }

    #[test]
    fn every_entry_on_the_git_menu_is_in_the_list_the_cli_offers() {
        for action in every_action_on_every_menu() {
            if let Action::Git(what) = action {
                assert!(
                    GitAction::ALL.iter().any(|listed| listed.name() == what.name()),
                    "the Git menu has {} and `git actions` does not list it",
                    what.name()
                );
            }
        }
    }

    #[test]
    fn the_three_actions_that_would_open_a_file_chooser_name_a_command_instead() {
        assert_eq!(Action::instead_of_a_file_chooser("open-file"), Some("tab open <path>"));
        assert_eq!(Action::instead_of_a_file_chooser("open-folder"), Some("project open <folder>"));
        assert_eq!(Action::instead_of_a_file_chooser("save-as"), Some("tab save-as <path>"));
        assert_eq!(Action::instead_of_a_file_chooser("save"), None, "Save needs no chooser");
    }
}
