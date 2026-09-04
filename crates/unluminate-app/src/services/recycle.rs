//! Where a deleted file goes.
//!
//! `task-1681` asks for a Delete on the explorer's menu and on the `Delete` key, with a question in
//! front of it. Every editor surveyed pairs that question with a **trash**, and the pairing is the
//! point: VS Code's dialog says "You can restore this file from the Recycle Bin", and that sentence
//! is what makes the question easy to answer. A confirmation on its own is a much weaker safety net
//! than a Recycle Bin, and the two together are what the reference editor, VS Code and Sublime all ship.
//!
//! So on **Windows** a deleted file goes to the Recycle Bin, through `SHFileOperationW` with
//! `FOF_ALLOWUNDO`. That is one feature flag on the `windows-sys` dependency Unluminate already has for
//! the window's transparency, and no new crate.
//!
//! **Everywhere else it is deleted outright**, and the question says so rather than pretending
//! otherwise. That divergence is a stated cost. The `trash` crate would close it and brings a
//! dependency tree per platform; macOS's own `NSFileManager.trashItemAtURL` would close it with no
//! new crate and cannot be compiled, let alone tested, on the machine Unluminate is built on.
//!
//! [`Destination`] is the enum the dialog's wording is derived from, so the day the second platform
//! is answered there is one place to change and the sentence follows.

use std::path::Path;

/// Where deleting puts a file on this platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// The platform's own undo bin, which the person can get the file back out of.
    Bin,
    /// Gone. Nothing in Unluminate can bring it back.
    Gone,
}

impl Destination {
    /// What the platform calls it, for the sentence in the confirmation.
    pub fn name(self) -> &'static str {
        match self {
            Destination::Bin => "the Recycle Bin",
            Destination::Gone => "nowhere",
        }
    }

    /// The reassurance, or the warning, that goes under the question.
    pub fn reassurance(self) -> &'static str {
        match self {
            Destination::Bin => "It goes to the Recycle Bin, so it can be got back.",
            Destination::Gone => "This cannot be undone.",
        }
    }
}

/// Where a deleted file goes on the platform this was built for.
pub const fn destination() -> Destination {
    if cfg!(windows) {
        Destination::Bin
    } else {
        Destination::Gone
    }
}

/// Delete a file or a folder, putting it wherever [`destination`] says.
///
/// A folder goes with everything under it, which is what the confirmation counted before asking.
pub fn delete(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not there", path.display()),
        ));
    }
    #[cfg(windows)]
    {
        return to_the_recycle_bin(path);
    }
    #[cfg(not(windows))]
    {
        if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

/// Hand the path to the shell's own file operation, which is what puts it in the Recycle Bin.
///
/// `pFrom` is a **double** null terminated list of names rather than one string, which is the one
/// thing about this interface that has to be got right: a single terminator makes the shell read
/// past the end of the buffer looking for the next name.
///
/// `FOF_NOCONFIRMATION` and `FOF_SILENT` are asked for because Unluminate has already asked the question
/// itself, and a second dialog from the shell would be the same question twice.
#[cfg(windows)]
fn to_the_recycle_bin(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let plain = unluminate_terminal::paths::plain(&full);
    let mut wide: Vec<u16> = plain.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: wide.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // Safe: every pointer is either null or into `wide`, which outlives the call, and the shell
    // reads the structure and does not keep it.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(std::io::Error::other(format!(
            "the Recycle Bin refused it (0x{result:X})"
        )));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(std::io::Error::other("it was not deleted"));
    }
    Ok(())
}

/// How many files are inside a folder, for the sentence the question asks.
///
/// The disk rather than the project's file list, because what is about to be deleted is what is
/// really there — including whatever a build wrote, which the explorer's search list leaves out.
/// Bounded, because a folder with a hundred thousand files in it should not make the dialog wait.
pub fn count_inside(folder: &Path, most: usize) -> usize {
    let mut found = 0usize;
    let mut folders = vec![folder.to_path_buf()];
    while let Some(here) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&here) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type().map(|kind| kind.is_dir()) {
                Ok(true) => folders.push(entry.path()),
                Ok(false) => found += 1,
                Err(_) => {}
            }
            if found >= most {
                return found;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join("unluminate-recycle").join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("inner")).expect("make the folder");
        std::fs::write(root.join("note.md"), "a note\n").expect("write a file");
        std::fs::write(root.join("inner/deep.txt"), "deep\n").expect("write another");
        root
    }

    #[test]
    fn a_file_is_deleted_and_a_missing_one_says_so() {
        let root = folder("one-file");
        delete(&root.join("note.md")).expect("delete it");
        assert!(!root.join("note.md").exists());
        let problem = delete(&root.join("note.md")).expect_err("it has gone");
        assert!(problem.to_string().contains("not there"));
    }

    #[test]
    fn a_folder_goes_with_everything_under_it() {
        let root = folder("whole-folder");
        assert_eq!(count_inside(&root, 100), 2);
        delete(&root.join("inner")).expect("delete the folder");
        assert!(!root.join("inner").exists());
        assert!(root.join("note.md").is_file(), "its sibling is untouched");
    }

    #[test]
    fn the_count_stops_where_it_is_asked_to() {
        let root = folder("counting");
        assert_eq!(count_inside(&root, 1), 1);
    }

    #[test]
    fn the_sentence_says_where_it_goes() {
        let sentence = destination().reassurance();
        assert!(sentence.ends_with('.'));
        if cfg!(windows) {
            assert_eq!(destination(), Destination::Bin);
            assert!(sentence.contains("Recycle Bin"));
        }
    }
}
