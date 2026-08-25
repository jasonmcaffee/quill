//! What was cut or copied in the explorer, waiting to be pasted.
//!
//! This is Quill's own clipboard, not the operating system's. The system's *file* clipboard is a
//! different interface on each platform — `CF_HDROP` on Windows, `NSFilenamesPboardType` on macOS —
//! and `arboard`, which Quill already uses for text, exposes neither. Adding a platform-specific
//! dependency for each platform to make cut and paste reach outside Quill is not worth it for
//! version one.
//!
//! So the consequence is stated plainly rather than hidden: **a file cut in Quill cannot be pasted
//! in Explorer or the Finder**, and one cut there cannot be pasted here. Copying a path *as text*
//! does go to the system clipboard, because that is just text and `arboard` does it already.
//!
//! Pasting is a copy or a move depending on which of the two put the path here, which is what every
//! file manager does.

use std::path::{Path, PathBuf};

/// Whether the file is to be copied or moved when it is pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

/// The path waiting to be pasted, and what is to happen to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileClipboard {
    held: Option<(PathBuf, Transfer)>,
}

impl FileClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cut(&mut self, path: impl Into<PathBuf>) {
        self.held = Some((path.into(), Transfer::Move));
    }

    pub fn copy(&mut self, path: impl Into<PathBuf>) {
        self.held = Some((path.into(), Transfer::Copy));
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    pub fn held(&self) -> Option<(&Path, Transfer)> {
        self.held.as_ref().map(|(path, transfer)| (path.as_path(), *transfer))
    }

    pub fn clear(&mut self) {
        self.held = None;
    }

    /// Put what is held into `folder`.
    ///
    /// Returns where it ended up. A name already taken in the destination gets a number added rather
    /// than overwriting what is there: pasting must never quietly destroy a file, and asking would
    /// mean a dialog inside a dialog.
    pub fn paste_into(&mut self, folder: &Path) -> std::io::Result<PathBuf> {
        let Some((source, transfer)) = self.held.clone() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "nothing has been cut or copied"));
        };
        if !source.exists() {
            self.clear();
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} is no longer there", source.display()),
            ));
        }
        let name = source
            .file_name()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "a path with no name"))?;
        let target = free_name(folder, &name.to_string_lossy());
        if source.is_dir() {
            copy_folder(&source, &target)?;
            if transfer == Transfer::Move {
                std::fs::remove_dir_all(&source)?;
            }
        } else {
            std::fs::copy(&source, &target)?;
            if transfer == Transfer::Move {
                std::fs::remove_file(&source)?;
            }
        }
        // A move happens once. A copy can be pasted into several folders, which is what a file
        // manager does and what makes copy worth having as well as cut.
        if transfer == Transfer::Move {
            self.clear();
        }
        Ok(target)
    }
}

/// A path in `folder` called `name`, or `name 2`, `name 3` and so on if that is taken.
///
/// The number goes before the extension, so `notes 2.md` rather than `notes.md 2`.
pub fn free_name(folder: &Path, name: &str) -> PathBuf {
    let candidate = folder.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        // A leading dot is the whole name of a hidden file, not an extension.
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    for number in 2..1000 {
        let candidate = folder.join(format!("{stem} {number}{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    folder.join(name)
}

/// Copy a folder and everything under it.
fn copy_folder(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join("quill-file-clipboard").join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("from")).expect("make from");
        std::fs::create_dir_all(root.join("to")).expect("make to");
        std::fs::write(root.join("from/note.md"), "a note\n").expect("write the file");
        root
    }

    #[test]
    fn a_copy_leaves_the_original_and_a_cut_does_not() {
        let root = folder("copy-and-cut");
        let mut clipboard = FileClipboard::new();
        clipboard.copy(root.join("from/note.md"));
        let pasted = clipboard.paste_into(&root.join("to")).expect("paste");
        assert_eq!(pasted, root.join("to/note.md"));
        assert!(root.join("from/note.md").is_file(), "a copy leaves the original where it was");
        assert!(!clipboard.is_empty(), "a copy can be pasted into a second folder");

        std::fs::create_dir_all(root.join("second")).expect("make second");
        clipboard.cut(root.join("from/note.md"));
        clipboard.paste_into(&root.join("second")).expect("paste");
        assert!(!root.join("from/note.md").exists(), "a cut moves the file");
        assert!(root.join("second/note.md").is_file());
        assert!(clipboard.is_empty(), "a move happens once");
    }

    #[test]
    fn pasting_over_a_name_that_is_taken_adds_a_number_rather_than_overwriting() {
        let root = folder("no-overwrite");
        std::fs::write(root.join("to/note.md"), "something else\n").expect("write the other one");
        let mut clipboard = FileClipboard::new();
        clipboard.copy(root.join("from/note.md"));
        let pasted = clipboard.paste_into(&root.join("to")).expect("paste");
        assert_eq!(pasted, root.join("to/note 2.md"), "the number goes before the extension");
        assert_eq!(
            std::fs::read_to_string(root.join("to/note.md")).expect("read"),
            "something else\n",
            "the file that was there is untouched"
        );
    }

    #[test]
    fn a_folder_is_pasted_with_everything_under_it() {
        let root = folder("whole-folder");
        std::fs::create_dir_all(root.join("from/inner")).expect("make inner");
        std::fs::write(root.join("from/inner/deep.txt"), "deep\n").expect("write deep");
        let mut clipboard = FileClipboard::new();
        clipboard.copy(root.join("from"));
        clipboard.paste_into(&root.join("to")).expect("paste");
        assert!(root.join("to/from/inner/deep.txt").is_file());
    }

    #[test]
    fn pasting_something_that_has_gone_says_so_and_forgets_it() {
        let root = folder("gone");
        let mut clipboard = FileClipboard::new();
        clipboard.cut(root.join("from/missing.md"));
        let problem = clipboard.paste_into(&root.join("to")).expect_err("it is not there");
        assert!(problem.to_string().contains("no longer there"));
        assert!(clipboard.is_empty());
    }

    #[test]
    fn a_hidden_file_keeps_its_whole_name() {
        let root = folder("hidden");
        std::fs::write(root.join("to/.quillrc"), "one\n").expect("write it");
        assert_eq!(free_name(&root.join("to"), ".quillrc"), root.join("to/.quillrc 2"));
    }
}
