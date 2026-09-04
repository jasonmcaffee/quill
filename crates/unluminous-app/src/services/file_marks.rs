//! What Unluminous remembers about the *files* in a project, kept beside the project like everything else.
//!
//! `services::project_state` remembers what is true of the **project** — which files were open,
//! which folders were expanded, whether the terminal was up. This remembers what is true of a
//! **file**, and `task-1663` asks for the first of those: the passages somebody has marked with a
//! colour. It is named for the thing rather than for highlights because a highlight is the first of
//! these rather than the only one there will ever be.
//!
//! ## Why it is shaped like this
//!
//! The ticket asks for "a highly performant mechanism to track these file metadata", so the shape is
//! the point rather than an implementation detail.
//!
//! - A `HashMap` keyed by path. A file with no marks is **absent**, so it costs nothing at all, and
//!   asking whether a file has any is one hash rather than a scan.
//! - Inside a file, `unluminous_core::Highlights` — sorted by where each mark starts and never
//!   overlapping — so the one under the pointer is a binary search and the handful on the screen is
//!   a binary search and a walk.
//! - **One file on the disk for the whole project**, `.unluminous/highlights.txt`, rather than one beside
//!   each source file. Six hundred source files would otherwise mean six hundred files to open when
//!   a project opens, a directory tree mirroring the project, and a rename that has to move a
//!   metadata file as well.
//! - It is read once when the project opens and written only when something changed, at the moment
//!   the project state is written — once the pointer is up, so dragging never writes. Nothing walks
//!   the project and nothing is watched.
//!
//! ## The format
//!
//! ```text
//! # The highlighted passages in this project. Written by Unluminous, and safe to delete.
//! 120 240 #E8C04A59 src/main.rs
//! 300 312 #489FF880 src/main.rs
//! ```
//!
//! Three tokens and then **the rest of the line is the path**, so a path with spaces in it needs no
//! quoting. Paths are written relative to the project wherever they are inside it, exactly as
//! `open-files.txt` writes them, so a project that is moved or checked out somewhere else still
//! opens with its marks. A line that cannot be read is skipped rather than taken as a reason to
//! refuse the whole file, which is the rule the settings file already keeps.
//!
//! Offsets are bytes, against the file as it was when Unluminous last had it. A file rewritten outside
//! Unluminous — a `git checkout`, another editor — moves its own bytes and Unluminous cannot know it, so the
//! marks may land in the wrong place; `Document::set_highlights` clamps them so the worst case is a
//! wrong colour rather than a crash. The alternative, storing the marked text or a hash of its
//! neighbourhood, is a second copy of parts of the file in a file people commit.
//!
//! ## Only the released binary reads or writes it
//!
//! [`FileMarks::load`] is called from `UnluminousApp::restore_project` and from nowhere else, exactly as
//! the project state is: a test must not touch a person's files, and a `.unluminous` folder written into
//! a screenshot test's own sample project would change what the explorer draws in the middle of a
//! test. A window a test builds still holds a `FileMarks` — an empty one, in memory — so every menu
//! entry and every command works in a test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use unluminous_core::highlights::{Highlight, Highlights, Rgba};

use crate::services::project_state;

/// The file inside `.unluminous` that holds them.
pub const FILE: &str = "highlights.txt";

/// How many marks are kept for one project. A project with more than this has a problem a metadata
/// file is not going to fix, and a list that grows without limit is a file that grows without limit.
const LIMIT: usize = 20_000;

/// What Unluminous remembers about the files in one project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileMarks {
    files: HashMap<PathBuf, Highlights>,
    /// Set when something changed since the last write, so a frame that changed nothing writes
    /// nothing. Not part of what two of these compare as, because it is about the disk rather than
    /// about the marks.
    dirty: bool,
}

impl FileMarks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read what was marked in `root`.
    pub fn load(root: &Path) -> Self {
        let file = project_state::folder(root).join(FILE);
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Self::new();
        };
        Self::parse(root, &text)
    }

    /// Read the file's text. Split from [`Self::load`] so it can be tested without a disk.
    pub fn parse(root: &Path, text: &str) -> Self {
        let mut marks = Self::new();
        let mut total = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((mark, path)) = read_line(line) else {
                continue; // a line that cannot be read is a line that is not there
            };
            if total >= LIMIT {
                break;
            }
            total += 1;
            marks
                .files
                .entry(project_state::absolute(root, &path))
                .or_default()
                .add(mark.range, mark.color);
        }
        marks
    }

    /// Write them down, if anything has changed since they were last written.
    ///
    /// A failure is reported on the error output and otherwise ignored, for the reason the settings
    /// file already gives: a read-only folder is not a reason to stop editing.
    pub fn save(&mut self, root: &Path) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let folder = project_state::folder(root);
        let file = folder.join(FILE);
        if self.files.values().all(unluminous_core::Highlights::is_empty) {
            // Nothing is marked any more. The file is emptied rather than left holding what was
            // cleared, and a project that never had one does not get one.
            if file.exists() {
                if let Err(problem) = std::fs::write(&file, heading()) {
                    eprintln!("Unluminous could not write {}: {problem}", file.display());
                }
            }
            return;
        }
        if let Err(problem) = std::fs::create_dir_all(&folder) {
            eprintln!("Unluminous could not make {}: {problem}", folder.display());
            return;
        }
        if let Err(problem) = std::fs::write(&file, self.to_text(root)) {
            eprintln!("Unluminous could not write {}: {problem}", file.display());
        }
    }

    /// What would be written. One line a mark, the files in a settled order so that two runs of
    /// Unluminous that marked the same passages write the same file.
    pub fn to_text(&self, root: &Path) -> String {
        let mut out = heading();
        for (path, marks) in self.files() {
            let written = project_state::relative(root, path);
            for mark in marks.iter() {
                out.push_str(&format!(
                    "{} {} {} {}\n",
                    mark.range.start,
                    mark.range.end,
                    mark.color.to_hex(),
                    written.display()
                ));
            }
        }
        out
    }

    /// What is marked in one file, if anything.
    pub fn highlights(&self, path: &Path) -> Option<&Highlights> {
        self.files.get(path).filter(|marks| !marks.is_empty())
    }

    /// Replace what is marked in one file.
    ///
    /// The one way in, so that nothing can change the marks without the file being written
    /// afterwards. An empty set is kept as an empty entry rather than removed, because that is what
    /// says "this file was cleared" to a `save` that has to empty the file on the disk.
    pub fn set(&mut self, path: &Path, highlights: Highlights) {
        if self.files.get(path) == Some(&highlights) {
            return;
        }
        if highlights.is_empty() && !self.files.contains_key(path) {
            return;
        }
        self.files.insert(path.to_path_buf(), highlights);
        self.dirty = true;
    }

    /// Change what is marked in one file in place, and say whether anything changed.
    pub fn change(&mut self, path: &Path, change: impl FnOnce(&mut Highlights)) -> bool {
        let mut marks = self.files.get(path).cloned().unwrap_or_default();
        let before = marks.clone();
        change(&mut marks);
        if before == marks {
            return false;
        }
        self.files.insert(path.to_path_buf(), marks);
        self.dirty = true;
        true
    }

    /// Take every mark away, everywhere. What `highlight clear --all` asks for.
    /// Forget everything marked in one file, which is what deleting it means.
    pub fn forget(&mut self, path: &Path) {
        if self.files.remove(path).is_some() {
            self.dirty = true;
        }
    }

    /// Follow a set of files that moved, so a marked passage is still marked at the new path.
    ///
    /// `task-1681` is the second place a closed file's bytes move; this is the first place one
    /// changes its name. The marks themselves are untouched, because a move does not shift a byte
    /// inside the file.
    pub fn moved(&mut self, moved: &[(PathBuf, PathBuf)]) {
        for (old, new) in moved {
            if let Some(marks) = self.files.remove(old) {
                self.files.insert(new.clone(), marks);
                self.dirty = true;
            }
        }
    }

    pub fn clear_all(&mut self) -> usize {
        let cleared = self.total();
        if cleared > 0 {
            for marks in self.files.values_mut() {
                marks.clear_all();
            }
            self.dirty = true;
        }
        cleared
    }

    /// The files that have anything marked in them, in path order so a listing is stable.
    pub fn files(&self) -> Vec<(&PathBuf, &Highlights)> {
        let mut out: Vec<(&PathBuf, &Highlights)> = self
            .files
            .iter()
            .filter(|(_, marks)| !marks.is_empty())
            .collect();
        out.sort_by(|left, right| left.0.cmp(right.0));
        out
    }

    /// How many marks there are, across every file.
    pub fn total(&self) -> usize {
        self.files.values().map(unluminous_core::Highlights::len).sum()
    }

    /// Set when something has changed and has not been written yet.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

fn heading() -> String {
    "# The highlighted passages in this project. Written by Unluminous, and safe to delete.\n".to_owned()
}

/// `<start> <end> <#rrggbbaa> <path>`, the path being the rest of the line.
fn read_line(line: &str) -> Option<(Highlight, PathBuf)> {
    let mut parts = line.splitn(4, ' ');
    let start: usize = parts.next()?.parse().ok()?;
    let end: usize = parts.next()?.parse().ok()?;
    let color = Rgba::parse(parts.next()?)?;
    let path = parts.next()?.trim();
    if path.is_empty() || start >= end {
        return None;
    }
    Some((Highlight { range: start..end, color }, PathBuf::from(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const YELLOW: Rgba = Rgba::new(0xE8, 0xC0, 0x4A, 0x59);
    const BLUE: Rgba = Rgba::new(0x48, 0x9F, 0xF8, 0x80);

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("src")).expect("make the project");
        root
    }

    fn marked(ranges: &[(usize, usize, Rgba)]) -> Highlights {
        let mut marks = Highlights::new();
        for (start, end, color) in ranges {
            marks.add(*start..*end, *color);
        }
        marks
    }

    #[test]
    fn a_project_that_has_never_been_marked_holds_nothing() {
        let root = project("unluminous-file-marks-fresh");
        let marks = FileMarks::load(&root);
        assert_eq!(marks.total(), 0);
        assert!(marks.highlights(&root.join("src/main.rs")).is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn what_was_marked_is_what_comes_back() {
        let root = project("unluminous-file-marks-round-trip");
        let mut marks = FileMarks::new();
        marks.set(&root.join("src/main.rs"), marked(&[(10, 20, YELLOW), (40, 50, BLUE)]));
        marks.set(&root.join("notes.md"), marked(&[(0, 8, YELLOW)]));
        marks.save(&root);

        let read = FileMarks::load(&root);
        assert_eq!(read.total(), 3);
        assert_eq!(read.highlights(&root.join("src/main.rs")).map(Highlights::len), Some(2));
        assert_eq!(
            read.highlights(&root.join("notes.md")).and_then(|marks| marks.at(3)).map(|m| m.color),
            Some(YELLOW)
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_paths_are_written_relative_so_a_project_that_moves_keeps_its_marks() {
        let root = project("unluminous-file-marks-relative");
        let mut marks = FileMarks::new();
        marks.set(&root.join("src/main.rs"), marked(&[(10, 20, YELLOW)]));
        let written = marks.to_text(&root);
        assert!(
            !written.contains(&root.display().to_string()),
            "the project's own path should not be in the file, which holds {written:?}"
        );
        assert!(written.contains("main.rs"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_path_with_spaces_in_it_needs_no_quoting() {
        let root = PathBuf::from("/project");
        let mut marks = FileMarks::new();
        marks.set(&root.join("my notes/chapter one.md"), marked(&[(3, 9, BLUE)]));
        let text = marks.to_text(&root);
        let read = FileMarks::parse(&root, &text);
        assert_eq!(read.total(), 1);
        assert!(read.highlights(&root.join("my notes/chapter one.md")).is_some());
    }

    #[test]
    fn a_line_that_cannot_be_read_is_skipped_and_the_rest_of_the_file_still_loads() {
        let root = PathBuf::from("/project");
        let text = "# a heading\n\
                    this is not a highlight\n\
                    10 20 #E8C04A59 src/main.rs\n\
                    30 nonsense #E8C04A59 src/main.rs\n\
                    40 30 #E8C04A59 src/main.rs\n\
                    50 60 not-a-colour src/main.rs\n\
                    70 80 #489FF880 src/main.rs\n";
        let marks = FileMarks::parse(&root, text);
        assert_eq!(marks.total(), 2, "the two readable lines, and no more");
    }

    #[test]
    fn nothing_is_written_until_something_changes() {
        let root = project("unluminous-file-marks-dirty");
        let mut marks = FileMarks::new();
        assert!(!marks.is_dirty());
        marks.set(&root.join("src/main.rs"), marked(&[(1, 2, YELLOW)]));
        assert!(marks.is_dirty());
        marks.save(&root);
        assert!(!marks.is_dirty());
        marks.set(&root.join("src/main.rs"), marked(&[(1, 2, YELLOW)]));
        assert!(!marks.is_dirty(), "setting the same marks again is not a change");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clearing_a_file_empties_it_on_the_disk_rather_than_leaving_what_was_cleared() {
        let root = project("unluminous-file-marks-cleared");
        let file = root.join("src/main.rs");
        let mut marks = FileMarks::new();
        marks.set(&file, marked(&[(10, 20, YELLOW)]));
        marks.save(&root);
        assert_eq!(FileMarks::load(&root).total(), 1);

        marks.set(&file, Highlights::new());
        marks.save(&root);
        assert_eq!(FileMarks::load(&root).total(), 0, "what was cleared does not come back");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_files_are_listed_in_a_settled_order() {
        let root = PathBuf::from("/project");
        let mut marks = FileMarks::new();
        for name in ["c.rs", "a.rs", "b.rs"] {
            marks.set(&root.join(name), marked(&[(1, 2, YELLOW)]));
        }
        let listed: Vec<String> =
            marks.files().iter().map(|(path, _)| path.display().to_string()).collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted);
    }

    #[test]
    fn changing_in_place_reports_whether_anything_moved() {
        let root = PathBuf::from("/project");
        let file = root.join("a.rs");
        let mut marks = FileMarks::new();
        assert!(marks.change(&file, |marks| marks.add(0..4, YELLOW)));
        assert!(!marks.change(&file, |marks| marks.add(0..4, YELLOW)));
        assert!(marks.change(&file, |marks| {
            marks.clear_all();
        }));
        assert_eq!(marks.total(), 0);
    }
}
