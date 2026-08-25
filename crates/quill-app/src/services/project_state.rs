//! What Quill remembers about one project, kept in a `.quill` folder inside the project itself.
//!
//! `services::store` already remembers what belongs to the person: the font, the opacity, the recent
//! projects. This remembers what belongs to the *project*: which files were open, which of them was
//! showing, which folders in the explorer were opened out, whether the terminal was up and how many
//! tabs it had. `task-1658` asks for it, and IntelliJ's `.idea` and Visual Studio Code's `.vscode` are
//! both the same idea — a folder beside the code rather than a file in the user's settings, so that
//! copying the project copies its state and two people working on the same folder do not fight over one
//! file.
//!
//! Three files, all plain text, in the format `services::store` already uses:
//!
//! - `.quill/workspace.conf` — the flags and the numbers.
//! - `.quill/open-files.txt` — one path a line, in tab order.
//! - `.quill/expanded-folders.txt` — one path a line.
//!
//! The two lists are files of their own rather than numbered names in the conf, because that is what
//! `recent.txt` already does with a list of paths and because a list of paths reads better one to a line
//! than as `files.open.07 = ...`.
//!
//! **Paths are written relative to the project** wherever they are inside it, so a project that is moved,
//! or checked out somewhere else on another machine, still opens the files it was left with. A path that
//! is somehow outside the project is written in full.
//!
//! A file that cannot be read is treated as a file that is not there, which is the rule the settings file
//! already keeps: a project opening with nothing restored is better than a project that will not open.

use std::path::{Path, PathBuf};

use crate::services::store::Values;

/// The folder inside the project that Quill keeps its state in.
pub const FOLDER: &str = ".quill";

const WORKSPACE_FILE: &str = "workspace.conf";
const OPEN_FILES_FILE: &str = "open-files.txt";
const EXPANDED_FILE: &str = "expanded-folders.txt";

/// How many files are remembered. A window with more tabs than this open has a problem the state file
/// is not going to fix, and a list that grows without limit is a file that grows without limit.
const OPEN_LIMIT: usize = 60;

/// What was left open in one project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectState {
    /// The files that were open, in tab order.
    pub open_files: Vec<PathBuf>,
    /// Which of them was showing.
    pub active_file: usize,
    /// The folders that were opened out in the explorer.
    pub expanded_folders: Vec<PathBuf>,
    /// False when the explorer had been put away.
    pub explorer_visible: bool,
    /// True when the terminal tile was showing.
    pub terminal_visible: bool,
    /// How many terminal tabs there were. The shells themselves cannot be brought back — what a
    /// program was doing when the window closed is gone — so what is restored is the same number of
    /// fresh shells in the project's own folder, which is what a person means by "my terminals were
    /// there".
    pub terminal_tabs: usize,
}

impl ProjectState {
    /// The state a project that has never been opened in Quill has.
    pub fn new() -> Self {
        Self { explorer_visible: true, ..Self::default() }
    }
}

/// Where the state folder for `root` is.
pub fn folder(root: &Path) -> PathBuf {
    root.join(FOLDER)
}

/// Read what was left open in `root`.
pub fn load(root: &Path) -> ProjectState {
    let folder = folder(root);
    let values = match std::fs::read_to_string(folder.join(WORKSPACE_FILE)) {
        Ok(text) => Values::parse(&text),
        Err(_) => Values::new(),
    };
    let mut state = ProjectState::new();
    if let Some(on) = values.flag("explorer.visible") {
        state.explorer_visible = on;
    }
    if let Some(on) = values.flag("terminal.visible") {
        state.terminal_visible = on;
    }
    if let Some(count) = values.number("terminal.tabs") {
        state.terminal_tabs = (count.max(0.0) as usize).min(16);
    }
    state.open_files = read_paths(root, &folder.join(OPEN_FILES_FILE));
    state.open_files.truncate(OPEN_LIMIT);
    // A file that has since been removed or renamed is left out, so a project does not open with a
    // tab pointing at nothing.
    state.open_files.retain(|path| path.is_file());
    state.expanded_folders = read_paths(root, &folder.join(EXPANDED_FILE));
    state.expanded_folders.retain(|path| path.is_dir());
    state.active_file = values
        .number("files.active")
        .map(|index| index.max(0.0) as usize)
        .unwrap_or(0)
        .min(state.open_files.len().saturating_sub(1));
    state
}

/// Write what is open in `root` now.
///
/// A failure is reported on the error output and otherwise ignored, for the reason the settings file
/// already gives: a read-only folder, or a full disk, is not a reason to stop editing. A project on a
/// disk that cannot be written to still works — it just does not remember.
pub fn save(root: &Path, state: &ProjectState) {
    let folder = folder(root);
    if let Err(problem) = std::fs::create_dir_all(&folder) {
        eprintln!("Quill could not make {}: {problem}", folder.display());
        return;
    }
    let mut values = Values::new();
    values.set("explorer.visible", flag(state.explorer_visible));
    values.set("terminal.visible", flag(state.terminal_visible));
    values.set("terminal.tabs", state.terminal_tabs.to_string());
    values.set("files.active", state.active_file.to_string());
    let heading = "# What Quill left open in this project. Written by Quill, and safe to delete.";
    write(&folder.join(WORKSPACE_FILE), &values.to_text_headed(heading));
    write(&folder.join(OPEN_FILES_FILE), &paths_text(root, &state.open_files));
    write(&folder.join(EXPANDED_FILE), &paths_text(root, &state.expanded_folders));
}

fn flag(on: bool) -> &'static str {
    if on {
        "true"
    } else {
        "false"
    }
}

fn write(path: &Path, text: &str) {
    if let Err(problem) = std::fs::write(path, text) {
        eprintln!("Quill could not write {}: {problem}", path.display());
    }
}

/// One path a line, relative to the project where it can be.
fn paths_text(root: &Path, paths: &[PathBuf]) -> String {
    paths
        .iter()
        .take(OPEN_LIMIT)
        .map(|path| format!("{}\n", relative(root, path).display()))
        .collect()
}

/// Read one path a line, turning each back into a full path.
fn read_paths(root: &Path, file: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = absolute(root, Path::new(line));
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// `path` as it is written down: relative to the project when it is inside it.
pub fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf())
}

/// The other way round: what a written path means now.
pub fn absolute(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("chapters")).expect("make the project");
        std::fs::write(root.join("readme.md"), "# readme\n").expect("write readme.md");
        std::fs::write(root.join("chapters/one.md"), "# one\n").expect("write one.md");
        root
    }

    #[test]
    fn a_project_that_has_never_been_opened_has_the_explorer_showing_and_nothing_else() {
        let root = project("quill-project-state-fresh");
        let state = load(&root);
        assert!(state.open_files.is_empty());
        assert!(state.expanded_folders.is_empty());
        assert!(state.explorer_visible, "the explorer starts open");
        assert!(!state.terminal_visible);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn what_was_open_is_what_comes_back() {
        let root = project("quill-project-state-round-trip");
        let state = ProjectState {
            open_files: vec![root.join("readme.md"), root.join("chapters/one.md")],
            active_file: 1,
            expanded_folders: vec![root.join("chapters")],
            explorer_visible: false,
            terminal_visible: true,
            terminal_tabs: 2,
        };
        save(&root, &state);
        assert_eq!(load(&root), state);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_paths_are_written_relative_so_a_project_that_moves_still_opens_its_files() {
        let root = project("quill-project-state-relative");
        save(
            &root,
            &ProjectState {
                open_files: vec![root.join("chapters/one.md")],
                ..ProjectState::new()
            },
        );
        let written = std::fs::read_to_string(folder(&root).join(OPEN_FILES_FILE)).expect("read it");
        assert!(
            !written.contains(&root.display().to_string()),
            "the project's own path should not be in the file, which holds {written:?}"
        );
        assert!(written.contains("one.md"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_that_has_since_gone_is_left_out_rather_than_opened_as_nothing() {
        let root = project("quill-project-state-missing");
        save(
            &root,
            &ProjectState {
                open_files: vec![root.join("readme.md"), root.join("gone.md")],
                active_file: 1,
                ..ProjectState::new()
            },
        );
        let state = load(&root);
        assert_eq!(state.open_files, vec![root.join("readme.md")]);
        assert_eq!(state.active_file, 0, "the index comes back inside the list that is left");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_state_folder_that_cannot_be_read_leaves_the_project_opening_normally() {
        let root = project("quill-project-state-rubbish");
        std::fs::create_dir_all(folder(&root)).expect("make the state folder");
        std::fs::write(folder(&root).join(WORKSPACE_FILE), "this is not a settings file")
            .expect("write rubbish");
        let state = load(&root);
        assert!(state.explorer_visible, "the defaults are what a file with nothing in it means");
        assert!(state.open_files.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
