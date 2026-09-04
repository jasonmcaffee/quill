//! The project's breakpoints, kept beside the project in `.unluminous/breakpoints.conf`.
//!
//! `services::file_marks` is the same idea for the marked passages, and this is its sibling rather
//! than a second answer: a `HashMap` keyed by path, a file with none **absent** so it costs nothing,
//! and `unluminous_core::Breakpoints` inside each — sorted by offset, so the one on a line is a binary
//! search and the handful on the screen is a binary search and a walk.
//!
//! The **format** is the numbered one `run-configurations.conf` established rather than the marks'
//! one line a mark, because a breakpoint has four things to say about itself and two of them are
//! free text that would have to be quoted:
//!
//! ```text
//! # The breakpoints in this project. Written by Unluminous, and safe to edit by hand.
//! breakpoint.1.path = src/main.rs
//! breakpoint.1.offset = 1204
//! breakpoint.1.enabled = true
//! breakpoint.2.path = backend/server.js
//! breakpoint.2.offset = 88
//! breakpoint.2.condition = attempts > 3
//! ```
//!
//! Read and written by [`crate::services::store::Values`] like every other file Unluminous writes. A block
//! missing `path` or `offset` is dropped **whole**, which is the `run.N.*` rule: a project that opens
//! with one breakpoint missing is better than a project that will not open.
//!
//! Paths are relative to the project wherever they are inside it, exactly as `open-files.txt` and
//! `highlights.txt` write them, so a project that moves keeps its breakpoints.
//!
//! Offsets are bytes against the file as Unluminous last had it. A file rewritten outside Unluminous moves its
//! own bytes and Unluminous cannot know it, so `Document::set_breakpoints` clamps them: the worst case is
//! a dot on the wrong line, and the adapter's `verified` answer then says so honestly rather than the
//! layout engine panicking on a range past the end of the rope.
//!
//! ## Only the released binary reads or writes it
//!
//! [`BreakpointStore::load`] is called from `UnluminousApp::restore_project` and nowhere else, exactly as
//! the marks and the project state are: a test must not touch a person's files, and a `.unluminous` folder
//! written into a screenshot test's sample project would change what the explorer draws in the middle
//! of a test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use unluminous_core::breakpoints::{Breakpoint, Breakpoints};

use crate::services::project_state;
use crate::services::project_state::DiskStamp;
use crate::services::store::Values;

/// The file inside `.unluminous` that holds them.
pub const FILE: &str = "breakpoints.conf";

/// How many breakpoints are kept for one project.
///
/// The marks' reasoning at the scale a breakpoint really has: a project with more than a thousand
/// places it stops has a problem a metadata file will not fix, and a list that grows without limit is
/// a file that grows without limit.
const LIMIT: usize = 1_000;

/// Every breakpoint in one project.
#[derive(Debug, Clone, Default)]
pub struct BreakpointStore {
    files: HashMap<PathBuf, Breakpoints>,
    /// Set when something changed since the last write, so a frame that changed nothing writes
    /// nothing. Not part of what two of these compare as, because it is about the disk.
    dirty: bool,
    /// What `breakpoints.conf` looked like when it was last read or written.
    ///
    /// `task-1794`: a breakpoint is a byte offset into a file, and a `git checkout` puts **both** the
    /// file and this store's own file back at once. With the window holding the offsets it had, the
    /// two disagree and the adapter is asked to bind a line that is no longer the line — which fails
    /// exactly as silently as the fault above it. So the disk-owned side is re-checked, which is the
    /// rule `OpenFile::the_file_changed_underneath` already keeps about a tab. Not part of what two
    /// of these compare as, for `dirty`'s reason.
    stamp: Option<DiskStamp>,
}

/// Two stores are the same when they hold the same breakpoints.
///
/// By hand rather than derived, because `dirty` and `stamp` are about the disk rather than about
/// what is set where — which is what the comment on `dirty` has said since it was written, and what
/// a derived comparison quietly did not do.
impl PartialEq for BreakpointStore {
    fn eq(&self, other: &Self) -> bool {
        self.files == other.files
    }
}

impl BreakpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read what `root` has.
    pub fn load(root: &Path) -> Self {
        let file = project_state::folder(root).join(FILE);
        // Measured **before** the read, so a write that lands between the two is noticed next time
        // rather than being stamped as already seen.
        let stamp = DiskStamp::of(&file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Self { stamp, ..Self::new() };
        };
        Self { stamp, ..Self::parse(root, &text) }
    }

    /// True when `breakpoints.conf` has been written by something other than this window.
    ///
    /// The question `OpenFile::the_file_changed_underneath` asks about a tab, asked about the
    /// project's own state — one `metadata` call, on the timer that already asks the explorer's
    /// folders the same thing. A file that has never been written and still is not answers no.
    pub fn changed_on_disk(&self, root: &Path) -> bool {
        // Something this window has not written yet is its own to write; adopting a file over the
        // top of it would throw away a breakpoint somebody has just set.
        if self.dirty {
            return false;
        }
        DiskStamp::of(&project_state::folder(root).join(FILE)) != self.stamp
    }

    /// Read `root`'s file again, replacing what is held.
    ///
    /// Everything is replaced rather than merged: the file on disk is the whole answer, so a
    /// breakpoint this window had and the file does not is one that was taken away — which is what a
    /// checkout of a branch that never had it means.
    pub fn reload(&mut self, root: &Path) {
        *self = Self::load(root);
    }

    /// Read the file's text. Split from [`Self::load`] so it can be tested without a disk.
    pub fn parse(root: &Path, text: &str) -> Self {
        let values = Values::parse(text);
        let mut store = Self::new();
        let mut total = 0;
        for index in 1..=LIMIT {
            let key = |field: &str| format!("breakpoint.{index}.{field}");
            // A block is only a block while it has both of the things that make it one. A run that
            // stops is the end of the list, so a file numbered 1, 2, 4 keeps the first two — the
            // same reading `run-configurations.conf` gets.
            let (Some(path), Some(offset)) = (
                values.text(&key("path")).map(str::trim).filter(|path| !path.is_empty()),
                values.text(&key("offset")).and_then(|value| value.trim().parse::<usize>().ok()),
            ) else {
                break;
            };
            total += 1;
            if total > LIMIT {
                break;
            }
            let breakpoint = Breakpoint {
                offset,
                // Missing means on, because a person editing the file by hand writes the line they
                // want to change and a breakpoint nobody said anything about is an ordinary one.
                enabled: values.flag(&key("enabled")).unwrap_or(true),
                condition: text_of(&values, &key("condition")),
                log_message: text_of(&values, &key("log")),
            };
            store
                .files
                .entry(project_state::absolute(root, Path::new(path)))
                .or_default()
                .set(breakpoint);
        }
        store
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
        if self.total() == 0 {
            // Nothing is left. The file is emptied rather than left holding what was taken away, and
            // a project that never had one does not get one — `FileMarks::save`'s rule.
            if file.exists() {
                if let Err(problem) = std::fs::write(&file, heading()) {
                    eprintln!("Unluminous could not write {}: {problem}", file.display());
                }
            }
            // Stamped after every write, so this window's own writing never reads as somebody else
            // having changed the file underneath it.
            self.stamp = DiskStamp::of(&file);
            return;
        }
        if let Err(problem) = std::fs::create_dir_all(&folder) {
            eprintln!("Unluminous could not make {}: {problem}", folder.display());
            return;
        }
        if let Err(problem) = std::fs::write(&file, self.to_text(root)) {
            eprintln!("Unluminous could not write {}: {problem}", file.display());
        }
        self.stamp = DiskStamp::of(&file);
    }

    /// What would be written, the files in a settled order so that two runs of Unluminous that put
    /// breakpoints in the same places write the same file.
    pub fn to_text(&self, root: &Path) -> String {
        let mut out = heading();
        let mut index = 0;
        for (path, breakpoints) in self.files() {
            let written = project_state::relative(root, path);
            for breakpoint in breakpoints.iter() {
                index += 1;
                out.push_str(&format!("breakpoint.{index}.path = {}\n", written.display()));
                out.push_str(&format!("breakpoint.{index}.offset = {}\n", breakpoint.offset));
                // Written only when it is not the ordinary answer, so a file of plain breakpoints
                // stays two lines each and reads at a glance.
                if !breakpoint.enabled {
                    out.push_str(&format!("breakpoint.{index}.enabled = false\n"));
                }
                if let Some(condition) = written_text(&breakpoint.condition) {
                    out.push_str(&format!("breakpoint.{index}.condition = {condition}\n"));
                }
                if let Some(message) = written_text(&breakpoint.log_message) {
                    out.push_str(&format!("breakpoint.{index}.log = {message}\n"));
                }
            }
        }
        out
    }

    /// What is set in one file, if anything.
    pub fn breakpoints(&self, path: &Path) -> Option<&Breakpoints> {
        self.files.get(path).filter(|set| !set.is_empty())
    }

    /// Replace what is set in one file.
    ///
    /// The one way in, so nothing can change a breakpoint without the file being written afterwards.
    /// An empty set is kept as an empty entry rather than removed, because that is what says "this
    /// file was cleared" to a `save` that has to empty the file on disk.
    pub fn set(&mut self, path: &Path, breakpoints: Breakpoints) {
        if self.files.get(path) == Some(&breakpoints) {
            return;
        }
        if breakpoints.is_empty() && !self.files.contains_key(path) {
            return;
        }
        self.files.insert(path.to_path_buf(), breakpoints);
        self.dirty = true;
    }

    /// Change what is set in one file in place, and say whether anything changed.
    pub fn change(&mut self, path: &Path, change: impl FnOnce(&mut Breakpoints)) -> bool {
        let mut breakpoints = self.files.get(path).cloned().unwrap_or_default();
        let before = breakpoints.clone();
        change(&mut breakpoints);
        if before == breakpoints {
            return false;
        }
        self.files.insert(path.to_path_buf(), breakpoints);
        self.dirty = true;
        true
    }

    /// Forget everything in one file, which is what deleting it means.
    pub fn forget(&mut self, path: &Path) {
        if self.files.remove(path).is_some() {
            self.dirty = true;
        }
    }

    /// Follow a set of files that moved, so a breakpoint is still there at the new path.
    ///
    /// The marks' rule: the offsets themselves are untouched, because a move does not shift a byte
    /// inside the file.
    pub fn moved(&mut self, moved: &[(PathBuf, PathBuf)]) {
        for (old, new) in moved {
            if let Some(breakpoints) = self.files.remove(old) {
                self.files.insert(new.clone(), breakpoints);
                self.dirty = true;
            }
        }
    }

    /// Take every breakpoint away, everywhere. How many there were.
    pub fn clear_all(&mut self) -> usize {
        let cleared = self.total();
        if cleared > 0 {
            for breakpoints in self.files.values_mut() {
                breakpoints.clear();
            }
            self.dirty = true;
        }
        cleared
    }

    /// The files that have any, in path order so a listing is stable.
    pub fn files(&self) -> Vec<(&PathBuf, &Breakpoints)> {
        let mut out: Vec<(&PathBuf, &Breakpoints)> =
            self.files.iter().filter(|(_, set)| !set.is_empty()).collect();
        out.sort_by(|left, right| left.0.cmp(right.0));
        out
    }

    /// How many there are, across every file.
    pub fn total(&self) -> usize {
        self.files.values().map(Breakpoints::len).sum()
    }

    /// Set when something has changed and has not been written yet.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

fn heading() -> String {
    "# The breakpoints in this project. Written by Unluminous, and safe to edit by hand.\n".to_owned()
}

/// A value that is there and has something in it. Blank is the same as absent, which is the rule
/// `SourceBreakpoint` keeps on the wire.
fn text_of(values: &Values, key: &str) -> Option<String> {
    values.text(key).map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// The same question about a value on its way out.
fn written_text(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { "C:\\project" } else { "/project" })
    }

    fn store_with(entries: &[(&str, Breakpoint)]) -> BreakpointStore {
        let mut store = BreakpointStore::new();
        for (path, breakpoint) in entries {
            let path = root().join(path);
            store.change(&path, |set| set.set(breakpoint.clone()));
        }
        store
    }

    #[test]
    fn what_is_written_reads_back_as_what_was_written() {
        let store = store_with(&[
            ("src/main.rs", Breakpoint::at(1204)),
            (
                "backend/server.js",
                Breakpoint {
                    offset: 88,
                    enabled: true,
                    condition: Some("attempts > 3".to_owned()),
                    log_message: None,
                },
            ),
            (
                "src/main.rs",
                Breakpoint { offset: 40, enabled: false, condition: None, log_message: None },
            ),
        ]);
        let text = store.to_text(&root());
        let read = BreakpointStore::parse(&root(), &text);
        assert_eq!(read.total(), 3);
        let main = read.breakpoints(&root().join("src/main.rs")).expect("two in main.rs");
        assert_eq!(main.len(), 2);
        assert!(!main.at(40).expect("the disabled one").enabled);
        assert!(main.at(1204).expect("the ordinary one").enabled);
        let server = read.breakpoints(&root().join("backend/server.js")).expect("one in server.js");
        assert_eq!(
            server.at(88).and_then(|one| one.condition.clone()),
            Some("attempts > 3".to_owned())
        );
    }

    /// The relative paths are what makes a project that moves keep its breakpoints.
    #[test]
    fn paths_are_written_relative_to_the_project() {
        let store = store_with(&[("src/main.rs", Breakpoint::at(4))]);
        let text = store.to_text(&root());
        assert!(text.contains("breakpoint.1.path = src"), "{text}");
        assert!(
            !text.lines().any(|line| line.contains(&root().display().to_string())),
            "nothing absolute reaches the file:\n{text}"
        );
        let elsewhere = PathBuf::from(if cfg!(windows) { "D:\\moved" } else { "/moved" });
        let read = BreakpointStore::parse(&elsewhere, &text);
        assert!(read.breakpoints(&elsewhere.join("src/main.rs")).is_some());
    }

    /// A block missing the two things that make it one is dropped whole, which is the `run.N.*`
    /// rule: a project that opens with one breakpoint missing beats one that will not open.
    #[test]
    fn a_block_missing_its_path_or_its_offset_is_dropped_whole() {
        let text = "breakpoint.1.path = a.rs\nbreakpoint.1.enabled = false\n";
        assert_eq!(BreakpointStore::parse(&root(), text).total(), 0, "no offset, no breakpoint");
        let text = "breakpoint.1.offset = 4\nbreakpoint.1.condition = x\n";
        assert_eq!(BreakpointStore::parse(&root(), text).total(), 0, "no path, no breakpoint");
    }

    #[test]
    fn a_run_that_stops_is_the_end_of_the_list() {
        let text = concat!(
            "breakpoint.1.path = a.rs\nbreakpoint.1.offset = 1\n",
            "breakpoint.2.path = b.rs\nbreakpoint.2.offset = 2\n",
            "breakpoint.4.path = d.rs\nbreakpoint.4.offset = 4\n",
        );
        let read = BreakpointStore::parse(&root(), text);
        assert_eq!(read.total(), 2, "the gap at three ends it, as it does for run configurations");
    }

    /// An ordinary breakpoint stays two lines, so a file of them reads at a glance.
    #[test]
    fn nothing_is_written_that_is_the_ordinary_answer() {
        let store = store_with(&[("a.rs", Breakpoint::at(7))]);
        let text = store.to_text(&root());
        assert!(!text.contains("enabled"), "on is the ordinary answer:\n{text}");
        assert!(!text.contains("condition"), "{text}");
        assert!(!text.contains("log"), "{text}");
        assert_eq!(text.lines().filter(|line| line.starts_with("breakpoint")).count(), 2);
    }

    #[test]
    fn a_blank_condition_is_not_written_and_reads_back_as_none() {
        let store = store_with(&[(
            "a.rs",
            Breakpoint {
                offset: 1,
                enabled: true,
                condition: Some("   ".to_owned()),
                log_message: Some(String::new()),
            },
        )]);
        let text = store.to_text(&root());
        assert!(!text.contains("condition"), "{text}");
        let read = BreakpointStore::parse(&root(), &text);
        assert!(read.breakpoints(&root().join("a.rs")).expect("there").at(1).expect("there").condition.is_none());
    }

    #[test]
    fn a_file_with_none_costs_nothing_and_is_not_listed() {
        let mut store = store_with(&[("a.rs", Breakpoint::at(1))]);
        assert!(store.breakpoints(&root().join("b.rs")).is_none());
        store.set(&root().join("b.rs"), Breakpoints::new());
        assert_eq!(store.files().len(), 1, "an empty set is not a file with breakpoints in it");
    }

    #[test]
    fn nothing_is_written_until_something_changed() {
        let mut store = BreakpointStore::new();
        assert!(!store.is_dirty());
        assert!(store.change(&root().join("a.rs"), |set| set.set(Breakpoint::at(1))));
        assert!(store.is_dirty());
        assert!(
            !store.change(&root().join("a.rs"), |set| set.set(Breakpoint::at(1))),
            "putting one where it already is changes nothing"
        );
    }

    /// A move does not shift a byte inside a file, so the offsets are untouched and only the name
    /// changes — which is the rule `FileMarks::moved` already keeps.
    #[test]
    fn a_file_that_moved_keeps_its_breakpoints_at_the_new_name() {
        let mut store = store_with(&[("src/old.rs", Breakpoint::at(12))]);
        store.moved(&[(root().join("src/old.rs"), root().join("src/new.rs"))]);
        assert!(store.breakpoints(&root().join("src/old.rs")).is_none());
        assert_eq!(
            store.breakpoints(&root().join("src/new.rs")).expect("moved with it").at(12),
            Some(&Breakpoint::at(12))
        );
    }

    #[test]
    fn forgetting_a_file_takes_its_breakpoints_with_it() {
        let mut store = store_with(&[("a.rs", Breakpoint::at(1))]);
        store.forget(&root().join("a.rs"));
        assert_eq!(store.total(), 0);
    }

    #[test]
    fn clearing_everything_says_how_many_there_were() {
        let mut store = store_with(&[("a.rs", Breakpoint::at(1)), ("b.rs", Breakpoint::at(2))]);
        assert_eq!(store.clear_all(), 2);
        assert_eq!(store.total(), 0);
        assert_eq!(store.clear_all(), 0);
    }

    /// A file written by hand is the format's whole reason for being readable, so an ordinary
    /// hand-written one has to work.
    #[test]
    fn a_file_somebody_wrote_by_hand_reads() {
        let text = concat!(
            "# mine\n",
            "breakpoint.1.path  =  src/main.rs\n",
            "breakpoint.1.offset= 1204\n",
            "breakpoint.1.enabled = no\n",
            "breakpoint.1.condition = attempts > 3\n",
            "breakpoint.1.log = seen {attempts}\n",
        );
        let read = BreakpointStore::parse(&root(), text);
        let one = read
            .breakpoints(&root().join("src/main.rs"))
            .expect("one")
            .at(1204)
            .expect("at 1204")
            .clone();
        assert!(!one.enabled, "`no` is one of the words the value store reads as false");
        assert_eq!(one.condition.as_deref(), Some("attempts > 3"));
        assert_eq!(one.log_message.as_deref(), Some("seen {attempts}"));
        assert!(one.is_conditional());
    }
}
