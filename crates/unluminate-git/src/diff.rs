//! What changed, both as text to read and as the lines the gutter draws a bar against.
//!
//! Two different things come out of one command. The commit panel shows a unified diff, which is the
//! text git prints. The gutter wants to know which *lines of the file as it is now* differ from the
//! version git has, which means reading the hunk headers and counting.
//!
//! A hunk header says where the hunk is on each side:
//!
//! ```text
//! @@ -12,7 +12,9 @@ async listByChat(chatId: number) {
//! ```
//!
//! `-12,7` is seven lines from line twelve of the old file and `+12,9` is nine lines from line
//! twelve of the new one. Only the new side matters here, because the gutter is drawn beside the
//! file as it is now.

use std::ffi::OsString;
use std::path::Path;

use crate::command::{run, Outcome};

/// How a line differs from the version git has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    /// The line is not in the version git has.
    Added,
    /// The line is there but different, or a line was deleted just before it.
    Modified,
}

/// The unified diff of a path.
///
/// `revision` compares against a commit instead of the working tree's own HEAD, which is
/// `Compare with Revision`. `staged` asks what a commit would hold rather than what is on disk.
pub fn of_path(folder: &Path, path: &Path, staged: bool, revision: Option<&str>) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["diff".into()];
    if staged {
        arguments.push("--cached".into());
    }
    if let Some(revision) = revision {
        arguments.push(revision.into());
    }
    arguments.push("--".into());
    arguments.push(path.into());
    run(folder, &arguments)
}

/// The unified diff of a whole commit.
pub fn of_commit(folder: &Path, hash: &str) -> Outcome {
    run(folder, &["show", "--stat", "--patch", hash])
}

/// Which lines of a file as it is now differ from the version git has.
pub fn changed_lines(folder: &Path, path: &Path) -> Vec<(usize, LineChange)> {
    let mut arguments: Vec<OsString> = vec!["diff".into(), "--unified=0".into(), "--".into()];
    arguments.push(path.into());
    let outcome = run(folder, &arguments);
    if !outcome.ok {
        return Vec::new();
    }
    let mut changes = parse_hunks(&outcome.stdout);
    if !changes.is_empty() {
        return changes;
    }
    // Nothing against HEAD can also mean the file is not tracked at all, in which case every line of
    // it is new. `git diff` says nothing about a file git has never seen.
    let known = run(folder, {
        let mut arguments: Vec<OsString> = vec!["ls-files".into(), "--error-unmatch".into(), "--".into()];
        arguments.push(path.into());
        &arguments.clone()
    });
    if !known.ok {
        if let Ok(text) = std::fs::read_to_string(path) {
            changes = (0..text.lines().count()).map(|line| (line, LineChange::Added)).collect();
        }
    }
    changes
}

/// Read the hunk headers of a unified diff into a list of changed lines on the new side.
///
/// Written against a string so that every shape of header can be tested without a repository.
pub fn parse_hunks(text: &str) -> Vec<(usize, LineChange)> {
    let mut changes = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some((old, rest)) = rest.split_once(' ') else {
            continue;
        };
        let new = rest.split(' ').next().unwrap_or_default();
        let Some((start, count)) = range(new.trim_start_matches('+')) else {
            continue;
        };
        let removed = range(old.trim_start_matches('-')).map(|(_, count)| count).unwrap_or(0);
        if count == 0 {
            // Lines were deleted and none added. There is no line to draw a bar against, so the one
            // that is now in their place is marked instead, which is what every editor does.
            let at = start.max(1) - 1;
            changes.push((at, LineChange::Modified));
            continue;
        }
        // A hunk that removed nothing is new text; one that removed something as well is a change.
        let kind = if removed == 0 { LineChange::Added } else { LineChange::Modified };
        for offset in 0..count {
            // Git counts lines from one and the layout counts paragraphs from zero.
            changes.push((start + offset - 1, kind));
        }
    }
    changes.sort_by_key(|(line, _)| *line);
    changes.dedup_by_key(|(line, _)| *line);
    changes
}

/// `12,9`, or just `12`, which means one line.
fn range(text: &str) -> Option<(usize, usize)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hunk_that_only_adds_lines_marks_them_as_added() {
        let changes = parse_hunks("@@ -12,0 +13,2 @@ context\n+one\n+two\n");
        assert_eq!(changes, vec![(12, LineChange::Added), (13, LineChange::Added)]);
    }

    #[test]
    fn a_hunk_that_replaces_lines_marks_them_as_modified() {
        let changes = parse_hunks("@@ -12,2 +12,2 @@\n-old\n+new\n");
        assert_eq!(changes, vec![(11, LineChange::Modified), (12, LineChange::Modified)]);
    }

    #[test]
    fn a_hunk_with_no_count_is_one_line() {
        let changes = parse_hunks("@@ -5 +5 @@\n-old\n+new\n");
        assert_eq!(changes, vec![(4, LineChange::Modified)]);
    }

    #[test]
    fn a_deletion_marks_the_line_that_took_its_place() {
        // Three lines removed and none added: there is no line of the new file to colour, so the
        // line now sitting there is marked.
        let changes = parse_hunks("@@ -12,3 +11,0 @@\n-one\n-two\n-three\n");
        assert_eq!(changes, vec![(10, LineChange::Modified)]);
    }

    #[test]
    fn a_deletion_at_the_very_top_of_a_file_does_not_run_off_the_start() {
        let changes = parse_hunks("@@ -1,2 +0,0 @@\n-one\n-two\n");
        assert_eq!(changes, vec![(0, LineChange::Modified)]);
    }

    #[test]
    fn several_hunks_come_back_in_order_with_no_line_counted_twice() {
        let text = "@@ -1,0 +2,1 @@\n+a\n@@ -40,1 +41,1 @@\n-b\n+c\n@@ -1,0 +2,1 @@\n+a\n";
        let changes = parse_hunks(text);
        assert_eq!(changes, vec![(1, LineChange::Added), (40, LineChange::Modified)]);
    }

    #[test]
    fn anything_that_is_not_a_hunk_header_is_ignored() {
        let text = "diff --git a/x b/x\nindex aaa..bbb 100644\n--- a/x\n+++ b/x\n";
        assert!(parse_hunks(text).is_empty());
    }
}
