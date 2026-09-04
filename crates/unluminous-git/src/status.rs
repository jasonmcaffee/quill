//! What has changed in the working tree, and where the branch stands against its upstream.
//!
//! Read from `git status --porcelain=v2 -z --branch --untracked-files=all`. Version two of the
//! porcelain format is the one git documents as being for programs to read: each record starts with
//! a character saying what kind of record it is, the fields are in a fixed order, and the whole
//! thing is separated by zero bytes so a path with a space or a newline in it needs no unpicking.
//! It has not changed since git 2.11.
//!
//! The record kinds, and what this reads out of each:
//!
//! ```text
//! # branch.head main                     the branch that is checked out
//! # branch.upstream origin/main          what it tracks
//! # branch.ab +2 -1                      two commits ahead, one behind
//! 1 .M N... 100644 100644 100644 <hash> <hash> notes.md        an ordinary change
//! 2 R. N... 100644 100644 100644 <hash> <hash> R100 new\0old   a rename: two paths
//! u UU N... ...                                                unmerged, a conflict
//! ? scratch.txt                                                git is not tracking it
//! ! target/                                                    ignored
//! ```

use std::path::Path;

use crate::command::{run, split_nul, Outcome};

/// What git thinks of one file, on one side.
///
/// The two sides are the index — what a commit would hold — and the working tree, which is what is
/// on disk. A file can be modified in both at once, which is why they are two fields rather than one
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Unchanged,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// Both sides changed it and git could not decide, which is a conflict.
    Unmerged,
    /// Git is not tracking it at all.
    Untracked,
    /// Git has been told to ignore it.
    Ignored,
}

impl State {
    fn from_code(code: char) -> Self {
        match code {
            'M' => State::Modified,
            'A' => State::Added,
            'D' => State::Deleted,
            'R' => State::Renamed,
            'C' => State::Copied,
            'U' => State::Unmerged,
            _ => State::Unchanged,
        }
    }

    /// The single letter a list shows against a file.
    pub fn letter(self) -> &'static str {
        match self {
            State::Unchanged => " ",
            State::Modified => "M",
            State::Added => "A",
            State::Deleted => "D",
            State::Renamed => "R",
            State::Copied => "C",
            State::Unmerged => "!",
            State::Untracked => "?",
            State::Ignored => "I",
        }
    }
}

/// One file git has something to say about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the repository's root, with forward slashes, which is how git spells a path on
    /// both platforms.
    pub path: String,
    /// Where it came from, when it was renamed or copied.
    pub from: Option<String>,
    /// What a commit would hold.
    pub index: State,
    /// What is on disk.
    pub worktree: State,
}

impl Entry {
    /// True when this change would go into a commit as things stand.
    pub fn staged(&self) -> bool {
        !matches!(self.index, State::Unchanged | State::Untracked | State::Ignored)
    }

    /// True when git is not tracking the file at all, which is the `Unversioned Files` group in the
    /// commit panel.
    pub fn untracked(&self) -> bool {
        self.index == State::Untracked
    }

    pub fn conflicted(&self) -> bool {
        self.index == State::Unmerged || self.worktree == State::Unmerged
    }

    /// The file's own name, without the folders in front of it.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The folders in front of the name, which a list shows dimmed after it.
    pub fn folder(&self) -> &str {
        match self.path.rfind('/') {
            Some(at) => &self.path[..at],
            None => "",
        }
    }
}

/// What git says about the working tree as a whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// The branch that is checked out, or `None` on a detached HEAD.
    pub branch: Option<String>,
    /// What the branch tracks, when it tracks anything.
    pub upstream: Option<String>,
    /// How many commits this branch has that its upstream does not.
    pub ahead: usize,
    /// How many its upstream has that this branch does not.
    pub behind: usize,
    pub entries: Vec<Entry>,
}

impl Status {
    /// Read the working tree.
    pub fn read(folder: &Path) -> Result<Self, Outcome> {
        let outcome = run(
            folder,
            &["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"],
        );
        if !outcome.ok {
            return Err(outcome);
        }
        Ok(parse(&outcome.stdout))
    }

    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many files a commit would take as things stand.
    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.staged()).count()
    }

    pub fn untracked_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.untracked()).count()
    }

    /// How many files have changes git already knows about, which is what the commit panel counts
    /// as `modified`.
    pub fn modified_count(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.untracked() && !entry.conflicted()).count()
    }

    pub fn conflicts(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|entry| entry.conflicted()).collect()
    }

    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    /// What the status bar says: the branch and how far it is from its upstream.
    pub fn branch_label(&self) -> String {
        let branch = self.branch.clone().unwrap_or_else(|| "detached HEAD".to_owned());
        match (self.ahead, self.behind) {
            (0, 0) => branch,
            (ahead, 0) => format!("{branch} \u{2191}{ahead}"),
            (0, behind) => format!("{branch} \u{2193}{behind}"),
            (ahead, behind) => format!("{branch} \u{2191}{ahead} \u{2193}{behind}"),
        }
    }
}

/// Turn the output of porcelain v2 into a [`Status`].
///
/// Written as a function of a string rather than of a folder, so that every record kind can be
/// tested against output written by hand as well as against a real repository.
pub fn parse(text: &str) -> Status {
    let mut status = Status::default();
    let records = split_nul(text);
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        let Some((kind, rest)) = record.split_once(' ') else {
            continue;
        };
        match kind {
            "#" => read_header(&mut status, rest),
            // An ordinary change, and a rename or copy, which carries a second path in a record of
            // its own straight after it.
            "1" | "2" => {
                let renamed = kind == "2";
                if let Some(mut entry) = read_change(rest, renamed) {
                    if renamed {
                        entry.from = records.get(index).map(|path| (*path).to_owned());
                        index += 1;
                    }
                    status.entries.push(entry);
                }
            }
            "u" => {
                if let Some(entry) = read_unmerged(rest) {
                    status.entries.push(entry);
                }
            }
            "?" => status.entries.push(Entry {
                path: rest.to_owned(),
                from: None,
                index: State::Untracked,
                worktree: State::Untracked,
            }),
            "!" => status.entries.push(Entry {
                path: rest.to_owned(),
                from: None,
                index: State::Ignored,
                worktree: State::Ignored,
            }),
            _ => {}
        }
    }
    status.entries.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    status
}

/// `# branch.head main`, `# branch.upstream origin/main`, `# branch.ab +2 -1`.
fn read_header(status: &mut Status, rest: &str) {
    let Some((name, value)) = rest.split_once(' ') else {
        return;
    };
    match name {
        // Git writes `(detached)` rather than a name when HEAD is not on a branch.
        "branch.head" => status.branch = (value != "(detached)").then(|| value.to_owned()),
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for part in value.split_whitespace() {
                let (sign, number) = part.split_at(1);
                let number: usize = number.parse().unwrap_or(0);
                match sign {
                    "+" => status.ahead = number,
                    "-" => status.behind = number,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// `<XY> <sub> <mH> <mI> <mW> <hH> <hI> [<score>] <path>`, with the score present only for a rename.
fn read_change(rest: &str, renamed: bool) -> Option<Entry> {
    let mut fields = rest.splitn(if renamed { 9 } else { 8 }, ' ');
    let codes: Vec<char> = fields.next()?.chars().collect();
    for _ in 0..(if renamed { 7 } else { 6 }) {
        fields.next()?;
    }
    let path = fields.next()?;
    Some(Entry {
        path: path.to_owned(),
        from: None,
        index: State::from_code(*codes.first()?),
        worktree: State::from_code(*codes.get(1)?),
    })
}

/// `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>` — a file both sides changed.
///
/// Ten fields after the `u`, not eleven: an unmerged entry carries three modes and three hashes,
/// one for each of the three sides of the conflict, and the mode on disk.
fn read_unmerged(rest: &str) -> Option<Entry> {
    let mut fields = rest.splitn(10, ' ');
    for _ in 0..9 {
        fields.next()?;
    }
    let path = fields.next()?;
    Some(Entry {
        path: path.to_owned(),
        from: None,
        index: State::Unmerged,
        worktree: State::Unmerged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records separated the way `-z` separates them.
    fn records(parts: &[&str]) -> String {
        let mut text = String::new();
        for part in parts {
            text.push_str(part);
            text.push('\0');
        }
        text
    }

    #[test]
    fn the_branch_and_how_far_it_is_from_its_upstream_are_read() {
        let status = parse(&records(&[
            "# branch.oid abc123",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
        ]));
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.branch_label(), "main \u{2191}2 \u{2193}1");
    }

    #[test]
    fn a_detached_head_has_no_branch_and_says_so() {
        let status = parse(&records(&["# branch.head (detached)"]));
        assert_eq!(status.branch, None);
        assert_eq!(status.branch_label(), "detached HEAD");
    }

    #[test]
    fn an_ordinary_change_is_read_on_both_sides() {
        // Staged in the index, changed again on disk.
        let status = parse(&records(&[
            "1 MM N... 100644 100644 100644 aaaa bbbb notes.md",
        ]));
        let entry = &status.entries[0];
        assert_eq!(entry.path, "notes.md");
        assert_eq!(entry.index, State::Modified);
        assert_eq!(entry.worktree, State::Modified);
        assert!(entry.staged());
        assert!(!entry.untracked());
    }

    #[test]
    fn a_change_only_on_disk_is_not_staged() {
        let status = parse(&records(&["1 .M N... 100644 100644 100644 aaaa bbbb notes.md"]));
        assert!(!status.entries[0].staged());
        assert_eq!(status.entries[0].index, State::Unchanged);
    }

    #[test]
    fn a_rename_carries_the_name_it_had_before() {
        let status = parse(&records(&[
            "2 R. N... 100644 100644 100644 aaaa bbbb R100 chapters/two.md",
            "chapters/one.md",
        ]));
        assert_eq!(status.entries.len(), 1, "the second record is the old path, not a second file");
        assert_eq!(status.entries[0].path, "chapters/two.md");
        assert_eq!(status.entries[0].from.as_deref(), Some("chapters/one.md"));
        assert_eq!(status.entries[0].index, State::Renamed);
    }

    #[test]
    fn a_conflict_is_reported_as_one() {
        let status = parse(&records(&[
            "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc notes.md",
        ]));
        assert!(status.entries[0].conflicted());
        assert_eq!(status.conflicts().len(), 1);
    }

    #[test]
    fn untracked_and_ignored_files_are_told_apart() {
        let status = parse(&records(&["? scratch.txt", "! target/"]));
        assert!(status.entry("scratch.txt").expect("scratch").untracked());
        assert_eq!(status.entry("target/").expect("target").index, State::Ignored);
        assert_eq!(status.untracked_count(), 1);
    }

    #[test]
    fn a_path_with_a_space_in_it_survives() {
        let status = parse(&records(&["1 .M N... 100644 100644 100644 aaaa bbbb my notes.md"]));
        assert_eq!(status.entries[0].path, "my notes.md", "the path is the rest of the record");
        assert_eq!(status.entries[0].name(), "my notes.md");
    }

    #[test]
    fn a_path_is_split_into_its_name_and_the_folders_in_front_of_it() {
        let entry = Entry {
            path: "backend/src/config/claudeProjects.config.ts".to_owned(),
            from: None,
            index: State::Modified,
            worktree: State::Unchanged,
        };
        assert_eq!(entry.name(), "claudeProjects.config.ts");
        assert_eq!(entry.folder(), "backend/src/config");
    }

    #[test]
    fn a_clean_tree_has_nothing_in_it() {
        let status = parse(&records(&["# branch.head main"]));
        assert!(status.is_clean());
        assert_eq!(status.branch_label(), "main");
    }

    #[test]
    fn entries_come_back_in_the_order_a_list_shows_them() {
        let status = parse(&records(&[
            "? Zebra.txt",
            "? apple.txt",
            "? Mango.txt",
        ]));
        let names: Vec<&str> = status.entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(names, vec!["apple.txt", "Mango.txt", "Zebra.txt"], "sorted ignoring case");
    }
}
