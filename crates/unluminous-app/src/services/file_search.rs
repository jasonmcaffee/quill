//! Finding a file by typing part of its name, which is what the `Go to File` modal is built on.
//!
//! `task-1659` asks for the reference editor's `Go to File`: a box to type in, a list that narrows as you type,
//! and a file opened from it. The matching is the whole of the interesting part, and it is here
//! rather than in the component so that it can be tested without a window — the component draws a
//! list of [`Found`] and knows nothing about how the list was arrived at.
//!
//! **The match is a subsequence, not a substring.** Typing `mdrs` finds `markdown.rs`, the way every
//! editor's file finder does, because nobody remembers the middle of a file name. A substring match
//! would find nothing at all for that, and a plain `contains` is what the explorer's filter already
//! offers for the times a substring is what you meant.
//!
//! **The name is worth more than the folder.** A query is nearly always a file's name, so a match in
//! the name outranks a match anywhere in the path, and `NAME_BONUS` is what that is worth. Without
//! it, typing `main` in a project with a `src/main` folder buries `main.rs` under everything inside
//! that folder.
//!
//! **Letters next to each other, and letters starting a word, are worth more than scattered ones.**
//! That is what makes `readme` put `readme.md` above `the-red-mesa.md`, and it is why the score is
//! built up per matched letter rather than being a count of them.

use std::path::{Path, PathBuf};

/// What a match in the file's name is worth over a match only in the folders above it.
const NAME_BONUS: i32 = 400;
/// What one matched letter is worth before any bonus.
const MATCH: i32 = 10;
/// A letter immediately after the previous matched one.
const ADJACENT: i32 = 14;
/// A letter at the start of a word: the first letter, or one after a separator or a case change.
const BOUNDARY: i32 = 16;
/// What each letter skipped over costs, so an early match beats a late one.
const SKIP: i32 = -2;

/// One file the query matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// The file's name, which is what the row shows first.
    pub name: String,
    /// Where the file is, relative to the project, which is what the row shows after the name. Empty
    /// for a file in the project's own folder.
    pub folder: String,
    /// How good the match is. Larger is better; the list is sorted by it.
    pub score: i32,
    /// Which characters of `name` matched, so the row can pick them out. Empty when the query
    /// matched the folders rather than the name.
    pub hits: Vec<usize>,
}

/// The files matching `query`, best first and no more than `limit` of them.
///
/// An empty query lists the project's files, shortest name first and then in path order, which is
/// what a `Go to File` box shows before anything is typed: something to look at and click, rather
/// than a blank panel.
pub fn find(root: &Path, files: &[PathBuf], query: &str, limit: usize) -> Vec<Found> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    let mut found: Vec<Found> = Vec::new();
    for path in files {
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let folder = folder_of(root, path);
        if needle.is_empty() {
            found.push(Found { path: path.clone(), name, folder, score: 0, hits: Vec::new() });
            continue;
        }
        // The name first, and the whole relative path only if the name did not match, so a file
        // whose name matches is never ranked by a coincidence in the folders above it.
        let relative = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
        match score(&name, &needle) {
            Some((score, hits)) => {
                found.push(Found { path: path.clone(), name, folder, score: score + NAME_BONUS, hits })
            }
            None => {
                if let Some((score, _)) = score(&relative, &needle) {
                    found.push(Found { path: path.clone(), name, folder, score, hits: Vec::new() });
                }
            }
        }
    }
    // Best first, and a tie broken by the shorter path, which is nearly always the one meant.
    found.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    found.truncate(limit);
    found
}

/// Where a file sits relative to the project, with forward slashes whatever the platform uses.
///
/// Forward slashes because the row shows this to a person beside a file name, and a mixture of
/// separators in one list reads as a fault. A file outside the project keeps its whole path.
pub fn folder_of(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    match relative.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.to_string_lossy().replace('\\', "/")
        }
        _ => String::new(),
    }
}

/// How well `text` matches `needle`, and which of its characters matched.
///
/// `needle` is already lower case. `None` means the letters are not in `text` in that order at all,
/// which is what takes a file out of the list.
///
/// The walk is greedy: each letter of the query takes the first place it fits. A greedy walk can
/// miss the best arrangement of a query in a long path — `ab` in `a-b-ab` matches the first `a` and
/// the second `b` rather than the pair at the end — and the exhaustive alternative is a matrix over
/// the whole path for every file on every key press. Greedy is what the list is sorted by, not what
/// decides whether a file appears at all, so the cost of being wrong is a row a place or two lower
/// than it might have been.
pub fn score(text: &str, needle: &[char]) -> Option<(i32, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    let characters: Vec<char> = text.chars().collect();
    let mut hits = Vec::with_capacity(needle.len());
    let mut total = 0;
    let mut at = 0;
    let mut previous: Option<usize> = None;
    for wanted in needle {
        let mut matched = None;
        while at < characters.len() {
            if characters[at].to_lowercase().eq(wanted.to_lowercase()) {
                matched = Some(at);
                break;
            }
            total += SKIP;
            at += 1;
        }
        let index = matched?;
        total += MATCH;
        if previous == Some(index.saturating_sub(1)) && index > 0 {
            total += ADJACENT;
        }
        if starts_a_word(&characters, index) {
            total += BOUNDARY;
        }
        hits.push(index);
        previous = Some(index);
        at = index + 1;
    }
    // A short name matching is a better answer than a long one matching, all else being equal.
    total -= characters.len() as i32 / 4;
    Some((total, hits))
}

/// True when the character at `index` starts a word: the first one, one after a separator, or a
/// capital in the middle of a name written in camel case.
fn starts_a_word(characters: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let before = characters[index - 1];
    if matches!(before, '/' | '\\' | '_' | '-' | '.' | ' ') {
        return true;
    }
    before.is_lowercase() && characters[index].is_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|name| PathBuf::from("/project").join(name)).collect()
    }

    fn names(found: &[Found]) -> Vec<String> {
        found.iter().map(|entry| entry.name.clone()).collect()
    }

    #[test]
    fn nothing_typed_lists_the_files_in_an_order_that_is_the_same_every_time() {
        let files = paths(&["readme.md", "notes.txt"]);
        let found = find(Path::new("/project"), &files, "", 10);
        assert_eq!(names(&found), vec!["notes.txt", "readme.md"], "nothing to rank by, so by path");
    }

    #[test]
    fn letters_are_matched_in_order_rather_than_as_a_substring() {
        let files = paths(&["markdown.rs", "notes.txt"]);
        let found = find(Path::new("/project"), &files, "mdrs", 10);
        assert_eq!(names(&found), vec!["markdown.rs"], "m-d-rs are all in markdown.rs, in that order");
    }

    #[test]
    fn a_file_whose_letters_are_not_in_order_is_not_offered() {
        let files = paths(&["readme.md"]);
        assert!(find(Path::new("/project"), &files, "zzz", 10).is_empty());
    }

    #[test]
    fn letters_next_to_each_other_beat_letters_scattered_about() {
        let files = paths(&["the-red-mesa.md", "readme.md"]);
        let found = find(Path::new("/project"), &files, "readme", 10);
        assert_eq!(names(&found)[0], "readme.md");
    }

    #[test]
    fn a_match_in_the_name_beats_a_match_in_the_folders_above_it() {
        let files = vec![
            PathBuf::from("/project/main/some-other-file.txt"),
            PathBuf::from("/project/src/main.rs"),
        ];
        let found = find(Path::new("/project"), &files, "main", 10);
        assert_eq!(names(&found)[0], "main.rs");
    }

    #[test]
    fn the_row_is_told_which_letters_matched_so_it_can_pick_them_out() {
        let files = paths(&["readme.md"]);
        let found = find(Path::new("/project"), &files, "rme", 10);
        assert_eq!(found[0].hits, vec![0, 4, 5], "the r of readme, and the m and e after it");
    }

    #[test]
    fn a_file_in_a_folder_says_which_folder_it_is_in() {
        let files = vec![PathBuf::from("/project/chapters/one.md"), PathBuf::from("/project/two.md")];
        let found = find(Path::new("/project"), &files, "", 10);
        assert_eq!(found[0].folder, "chapters");
        assert_eq!(found[1].folder, "", "a file in the project's own folder has no folder to name");
    }

    #[test]
    fn the_list_is_capped_so_a_very_large_project_still_answers_at_once() {
        let many: Vec<PathBuf> =
            (0..500).map(|index| PathBuf::from(format!("/project/file{index}.md"))).collect();
        assert_eq!(find(Path::new("/project"), &many, "file", 25).len(), 25);
    }

    #[test]
    fn matching_ignores_case_in_both_directions() {
        let files = paths(&["README.md"]);
        assert_eq!(find(Path::new("/project"), &files, "readme", 10).len(), 1);
        let files = paths(&["readme.md"]);
        assert_eq!(find(Path::new("/project"), &files, "README", 10).len(), 1);
    }

    #[test]
    fn a_capital_in_the_middle_of_a_name_starts_a_word() {
        let characters: Vec<char> = "sqlClient.ts".chars().collect();
        assert!(starts_a_word(&characters, 3), "the C of Client");
        assert!(!starts_a_word(&characters, 2));
    }
}
