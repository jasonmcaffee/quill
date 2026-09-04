//! A path as the programs on this machine can read one.
//!
//! `std::fs::canonicalize` on Windows does not give back `C:\jason\dev\unluminate`. It gives back
//! `\\?\C:\jason\dev\unluminate` — a **verbatim** path, the form that lets a path be longer than 260
//! characters and that is handed to the file system with none of the parsing every other path goes
//! through. Rust's own file calls take it happily, so nothing inside Unluminate notices, and it travels: into
//! the recent projects file, onto the explorer's root, and from there to the working directory a shell
//! is started in.
//!
//! `cmd.exe` refuses it. It reads the two leading backslashes as the start of a network share, says
//!
//! ```text
//! '\\?\C:\jason\dev\unluminate'
//! CMD.EXE was started with the above path as the current directory.
//! UNC paths are not supported.  Defaulting to Windows directory.
//! ```
//!
//! and starts in `C:\Windows` instead — which is `task-1670`, and is worse than an error, because the
//! terminal opens and works and is simply in the wrong folder.
//!
//! So this lives beside the code that starts other programs rather than in the window, because the
//! window is not the only thing that can hand a directory over: a test, an example and
//! `unluminate-cli terminal` all can, and a list of the places that have to remember to strip it is a list
//! whose next entry will be the one that forgot. The window's own store uses it as well, so that a
//! verbatim path is never written down in the first place.

use std::path::{Path, PathBuf};

/// The prefix Windows puts in front of a verbatim path.
const VERBATIM: &str = r"\\?\";
/// What a verbatim path uses in place of the two leading backslashes of a network share.
const VERBATIM_SHARE: &str = r"\\?\UNC\";

/// `path` with the verbatim prefix taken off, so that a program started in it can read it.
///
/// A path that is already plain, and every path on a system that has no such prefix, comes back
/// unchanged. `\\?\UNC\server\share` becomes `\\server\share`, which is the ordinary spelling of the
/// same place: a real network share is still a network share, and only the prefix is Windows' own.
///
/// A drive letter is what makes the short form safe. `\\?\Volume{...}` names a volume that has no letter
/// and has no shorter spelling, so it is left as it is rather than turned into something that names
/// somewhere else.
pub fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(VERBATIM_SHARE) {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    let Some(rest) = text.strip_prefix(VERBATIM) else {
        return path.to_path_buf();
    };
    if is_drive_path(rest) {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// `path` written with this platform's own separator throughout, and with `.` segments dropped.
///
/// `Path::join` does not normalise, so on Windows `root.join("src/report.rs")` is
/// `C:\project\src/report.rs` — a path with both separators in it. Nothing inside Unluminate notices,
/// because `Path`'s own `Eq` and `Hash` compare *components* and read `/` and `\` alike; and nothing
/// on the disk notices either, because Windows accepts both. It is only noticed by **another
/// program**, which is `plain`'s situation exactly: `task-1794` measured a breakpoint in a file that
/// was not open being sent to a debug adapter as `C:\project\src/report.rs` — accepted without
/// complaint, matched against no compile unit, and so never bound, with the program running to
/// completion and the debug tile simply staying empty.
///
/// It is `Path`'s own normalisation rather than a text substitution, so the path Unluminate hands over is
/// the path Unluminate compares. On a platform where `\` is an ordinary character in a filename nothing
/// is replaced, because there `components()` never splits on one.
///
/// A `..` segment is **kept**: removing one without touching the disk changes which file is named
/// whenever the segment before it is a symbolic link. This tidies the spelling and resolves nothing.
pub fn native(path: &Path) -> PathBuf {
    path.components().collect()
}

/// Whether `rest` starts with a drive letter and a colon, which is what a verbatim path has to hold
/// before the prefix can be taken off it.
fn is_drive_path(rest: &str) -> bool {
    let mut characters = rest.chars();
    let letter = characters.next();
    let colon = characters.next();
    matches!((letter, colon), (Some(letter), Some(':')) if letter.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_path_loses_its_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\jason\dev\unluminate")), PathBuf::from(r"C:\jason\dev\unluminate"));
        assert_eq!(plain(Path::new(r"\\?\c:\")), PathBuf::from(r"c:\"));
    }

    /// `task-1794`: the mixed-separator path a `join` makes is what a debug adapter was handed.
    #[test]
    fn a_path_written_with_both_separators_comes_back_with_one() {
        let joined = Path::new(if cfg!(windows) { r"C:\project" } else { "/project" })
            .join("src/report.rs");
        let wanted = if cfg!(windows) { r"C:\project\src\report.rs" } else { "/project/src/report.rs" };
        assert_eq!(native(&joined), PathBuf::from(wanted));
    }

    #[test]
    fn a_path_that_is_already_native_is_unchanged() {
        for path in [
            if cfg!(windows) { r"C:\project\src\report.rs" } else { "/project/src/report.rs" },
            "relative/bit",
        ] {
            assert_eq!(native(Path::new(path)), PathBuf::from(path), "{path}");
        }
    }

    /// A `.` says nothing and goes; a `..` decides which file is named and stays, because resolving
    /// one without reading the disk is wrong wherever the segment before it is a symbolic link.
    #[test]
    fn a_here_segment_is_dropped_and_an_up_segment_is_kept() {
        let root = Path::new(if cfg!(windows) { r"C:\project" } else { "/project" });
        assert_eq!(native(&root.join("./src/report.rs")), native(&root.join("src/report.rs")));
        assert!(native(&root.join("../other/report.rs")).to_string_lossy().contains(".."));
    }

    /// The whole point of it: what is normalised is what `Path` itself already compares, so a path
    /// handed to another program and a path compared inside Unluminate cannot come apart.
    #[test]
    fn normalising_never_changes_which_file_a_path_names() {
        let root = Path::new(if cfg!(windows) { r"C:\project" } else { "/project" });
        for path in [root.join("src/report.rs"), root.join("./a/b"), PathBuf::from("rel/x.rs")] {
            assert_eq!(native(&path), path, "{}", path.display());
        }
    }

    #[test]
    fn an_ordinary_path_is_left_alone() {
        for path in [r"C:\jason\dev\unluminate", "/home/jason/unluminate", r"\\server\share\folder", "relative/bit"]
        {
            assert_eq!(plain(Path::new(path)), PathBuf::from(path), "{path} should not change");
        }
    }

    #[test]
    fn a_verbatim_share_is_written_the_way_a_share_is_written() {
        assert_eq!(plain(Path::new(r"\\?\UNC\server\share\folder")), PathBuf::from(r"\\server\share\folder"));
    }

    #[test]
    fn a_verbatim_path_with_no_drive_letter_is_left_alone() {
        // A volume with no letter has no shorter spelling, so taking the prefix off would name nowhere.
        let volume = r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\folder";
        assert_eq!(plain(Path::new(volume)), PathBuf::from(volume));
    }

    #[test]
    fn stripping_twice_is_stripping_once() {
        let once = plain(Path::new(r"\\?\C:\jason\dev\unluminate"));
        assert_eq!(plain(&once), once, "a plain path stays plain");
    }
}
