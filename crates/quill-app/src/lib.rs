//! The Quill window.
//!
//! The editor itself is in `quill-core`, which has no user interface dependencies. This crate supplies
//! a window, input, painting, and real fonts behind the editor's measurements.
//!
//! The folders, and what belongs in each one. A later change should keep to this.
//!
//! - `app` is the window's own state and the actions the menus and the keyboard ask for.
//! - `components` draws: one file per piece of the window, each taking a rectangle and returning what
//!   the user did in it rather than changing the state itself.
//! - `services` is everything that is not drawing: the file tree, the fonts and the glyph atlas, the
//!   settings and recent projects on disk, starting a second window, and the macOS menu bar.
//! - `theme` is the palette, the measurements and the drawn icons.
//! - `build_info` is what this build is: the version and the date it was built.

pub mod app;
pub mod build_info;
pub mod components;
pub mod services;
pub mod settings;
pub mod theme;

pub use app::{QuillApp, ViewMode};
pub use services::file_tree::FileTree;
pub use services::text_renderer::TextRenderer;
pub use settings::Settings;

use std::path::{Path, PathBuf};

/// Work out what to show from the path given on the command line.
///
/// A file argument opens that file and shows the folder it sits in. A folder argument shows that folder. No
/// argument at all shows `fallback`, which the binary passes as the current directory.
///
/// This lives here rather than in `main.rs` so that it can be tested. It looked wrong once when a
/// translucent window let the folder tree of another application show through Quill's explorer, and the only
/// way to settle whether the resolution was at fault was to be able to run it.
pub fn resolve_target(argument: Option<&Path>, fallback: &Path) -> (PathBuf, Option<PathBuf>) {
    match argument {
        Some(path) if path.is_file() => {
            let folder = path.parent().filter(|parent| !parent.as_os_str().is_empty());
            (folder.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")), Some(path.to_path_buf()))
        }
        Some(path) => (path.to_path_buf(), None),
        None => (fallback.to_path_buf(), None),
    }
}

/// The folder to show when nothing was named on the command line.
///
/// `current_directory` is the honest answer when a person typed `quill` in a terminal: they are standing
/// in the folder they mean. It is not an answer at all when Quill was started from the desktop, the Start
/// menu or a file association, because then the current directory is whatever the shortcut points at —
/// which for Quill's own installer is the folder `quill.exe` sits in. `task-1670` is what that looked
/// like: quit Quill with a project open, start it again from the desktop, and the explorer and the
/// terminal both came up in `AppData\Local\Programs\Quill`.
///
/// So: when the current directory is the folder the program itself lives in, nobody chose it, and the
/// project that was open last time is what was meant. Otherwise the current directory stands. That is a
/// narrower rule than "always reopen the last project", and deliberately — `quill` typed in a folder has
/// to open *that* folder, or the command line would be lying about what it does.
///
/// `most_recent` is the head of the recent projects list, and `program` is `std::env::current_exe`. Both
/// are passed in rather than read here, so this is a rule that can be tested rather than a rule that
/// depends on where the test binary happens to live.
pub fn starting_folder(
    current_directory: &Path,
    program: Option<&Path>,
    most_recent: Option<&Path>,
) -> PathBuf {
    let installed_here = program
        .and_then(|program| program.parent())
        .is_some_and(|folder| same_folder(folder, current_directory));
    match most_recent {
        Some(project) if installed_here && project.is_dir() => project.to_path_buf(),
        _ => current_directory.to_path_buf(),
    }
}

/// Whether two paths name the same folder, as far as this can be told without asking the disk twice.
///
/// Compared through `canonicalize` when both resolve, because one of them has come from the operating
/// system and the other from the person, and `C:\Quill` and `C:\quill\` are the same folder.
fn same_folder(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PathBuf {
        let root = std::env::temp_dir().join("quill-resolve-target");
        std::fs::create_dir_all(root.join("inner")).expect("make the folder");
        std::fs::write(root.join("inner/note.md"), "# note\n").expect("write the file");
        root
    }

    #[test]
    fn a_file_shows_the_folder_it_sits_in_and_opens_the_file() {
        let root = sample();
        let file = root.join("inner/note.md");
        let (folder, opened) = resolve_target(Some(&file), Path::new("/nowhere"));
        assert_eq!(folder, root.join("inner"), "the explorer shows the folder holding the file");
        assert_eq!(opened, Some(file));
    }

    #[test]
    fn a_folder_is_shown_with_nothing_opened() {
        let root = sample();
        let (folder, opened) = resolve_target(Some(&root), Path::new("/nowhere"));
        assert_eq!(folder, root);
        assert_eq!(opened, None);
    }

    #[test]
    fn no_argument_falls_back_to_the_current_directory() {
        let (folder, opened) = resolve_target(None, Path::new("/some/where"));
        assert_eq!(folder, Path::new("/some/where"));
        assert_eq!(opened, None);
    }

    #[test]
    fn a_file_with_no_folder_in_front_of_it_resolves_to_the_current_directory() {
        // `quill note.md` run inside the folder holding it: `parent` is empty rather than absent, and an
        // empty path is not a folder anything can be read from.
        let root = sample();
        let previous = std::env::current_dir().expect("read the current directory");
        std::env::set_current_dir(root.join("inner")).expect("move into the folder");
        let (folder, opened) = resolve_target(Some(Path::new("note.md")), Path::new("/nowhere"));
        std::env::set_current_dir(previous).expect("move back");
        assert_eq!(folder, Path::new("."), "the folder is the current directory, not an empty path");
        assert_eq!(opened, Some(PathBuf::from("note.md")));
    }

    #[test]
    fn started_from_the_desktop_shows_the_project_that_was_open_last_time() {
        // `task-1670`: the current directory is then the folder holding `quill.exe`, which nobody chose.
        let root = sample();
        let program = root.join("Programs/Quill/quill.exe");
        std::fs::create_dir_all(program.parent().expect("a folder")).expect("make the folder");
        let installed = program.parent().expect("a folder").to_path_buf();
        let project = root.join("inner");

        assert_eq!(
            starting_folder(&installed, Some(&program), Some(&project)),
            project,
            "the last project is what was meant"
        );
        assert_eq!(
            starting_folder(&installed, Some(&program), None),
            installed,
            "with nothing opened before, there is nothing better than where it started"
        );
    }

    #[test]
    fn started_from_a_terminal_shows_the_folder_the_person_is_standing_in() {
        // The rule has to be this narrow, or `quill` typed in a folder would open a different one.
        let root = sample();
        let program = root.join("Programs/Quill/quill.exe");
        std::fs::create_dir_all(program.parent().expect("a folder")).expect("make the folder");
        let here = root.join("inner");
        let project = root.to_path_buf();
        assert_eq!(starting_folder(&here, Some(&program), Some(&project)), here);
    }

    #[test]
    fn a_last_project_that_has_gone_leaves_the_current_directory() {
        let root = sample();
        let program = root.join("Programs/Quill/quill.exe");
        std::fs::create_dir_all(program.parent().expect("a folder")).expect("make the folder");
        let installed = program.parent().expect("a folder").to_path_buf();
        let missing = root.join("a-project-that-was-deleted");
        std::fs::remove_dir_all(&missing).ok();
        assert_eq!(starting_folder(&installed, Some(&program), Some(&missing)), installed);
    }

    #[test]
    fn a_path_that_does_not_exist_is_treated_as_a_folder_to_show() {
        // The explorer then reports that it cannot be read, rather than Quill refusing to start.
        let missing = std::env::temp_dir().join("quill-resolve-target-missing");
        std::fs::remove_dir_all(&missing).ok();
        let (folder, opened) = resolve_target(Some(&missing), Path::new("/nowhere"));
        assert_eq!(folder, missing);
        assert_eq!(opened, None);
    }
}
