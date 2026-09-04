//! Which files the project index leaves out.
//!
//! `task-1804` §7.3. The only thing the index skipped was three hardcoded folder names, and
//! `.gitignore` was never read at all, so on this repository:
//!
//! ```text
//! $ unluminous-cli --json editor definition relayout
//! 'relayout' has 2 candidate definitions, both "sure":
//!   crates/unluminous-core/src/layout.rs
//!   _agent_output/task-1701-git-root-refresh/release-worktree/crates/quill-core/src/layout.rs
//! ```
//!
//! The second is a gitignored scratch copy of the whole project under its previous name. The same
//! list feeds `Go to File`, `Find in Files`, the symbol index, references and completion, so a
//! vendored dependency, a `dist/`, a `.venv/`, a `coverage/` or a second checkout inside the project
//! polluted all five — and `editor references relayout` taking 2,811 ms on this project was partly
//! that.
//!
//! **The explorer still shows everything**, which is that panel's own rule: it is a picture of the
//! folder and hiding most of one is exactly what `file_tree`'s comment at the top refuses to do.
//! What is filtered is [`crate::services::file_tree::FileTree::all_files`], which is the list the
//! five searches read.
//!
//! ## Why the file rather than a longer list of names
//!
//! The comment beside `is_build_output` deliberately left `build`, `dist` and `out` out, on the
//! reasoning that they are real folders in real projects and a search that silently missed a file
//! would be worse than one offering a few too many. That reasoning is right, and it argues *for*
//! reading `.gitignore` rather than for a longer hardcoded list: **the file says which of them this
//! project means.** A project that ignores `dist` has said so; one that keeps its `dist` in git has
//! said that too.
//!
//! ## What of the syntax is implemented, and what is not
//!
//! Enough of `gitignore(5)` that the answer matches git on the files anybody has: comments, blank
//! lines, negation with `!`, anchoring with a leading or interior `/`, a directory-only trailing `/`,
//! `*` and `?` within one path component, `**` across components, and character classes.
//!
//! Deliberately **not** implemented, and each is a decision rather than an omission:
//!
//! - **`.gitignore` files in subfolders.** Only the root's is read, plus `.git/info/exclude`. A
//!   nested file would want the walk to carry a stack of rule sets, and the pollution this was
//!   filed about is a root-level rule in every case measured. It is the next thing to add here.
//! - **`$GIT_DIR/info/exclude` outside the project, and `core.excludesFile`.** Those are the
//!   person's own global rules and reading them would mean a search that answers differently on two
//!   machines with the same checkout.
//! - **`.gitignore`'s "a directory that is excluded cannot have anything re-included from it"
//!   rule.** A negation is honoured wherever it appears here, which is more permissive than git —
//!   it offers a file rather than hiding one, which is the direction this file errs in throughout.

use std::path::{Path, PathBuf};

/// Folders holding what a build wrote rather than what a person did.
///
/// **The fallback for a folder that is not a repository**, which is what these three always were.
/// Inside a repository `.gitignore` is what decides, because the project has said.
///
/// Three names only, and each is a folder nobody writes a source file into. `build`, `dist` and
/// `out` are not here on purpose: see the note above.
pub const BUILD_OUTPUT: &[&str] = &["target", "node_modules", "__pycache__"];

/// One line of a `.gitignore`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    /// The pattern with its `!`, its leading `/` and its trailing `/` taken off.
    pattern: String,
    /// `!foo` — this rule brings a file back rather than leaving it out.
    negated: bool,
    /// `foo/` — it matches a directory and nothing else.
    directory_only: bool,
    /// `/foo` or `a/b` — it is measured from the root rather than against any component.
    anchored: bool,
}

/// What a project leaves out of its index.
#[derive(Debug, Clone, Default)]
pub struct Ignores {
    rules: Vec<Rule>,
    /// True when the root is a repository, which decides whether [`BUILD_OUTPUT`] applies.
    repository: bool,
}

impl Ignores {
    /// Read what `root` says to ignore, plus `extra` — the `editor.exclude` setting.
    ///
    /// `extra` is one line of comma separated patterns in the same syntax, because that is the
    /// syntax the person already knows and the one the reader beside it implements. It is read
    /// **after** the files, so a person's own line can bring a file back with `!` that the project
    /// left out.
    pub fn read(root: &Path, extra: &str) -> Self {
        let repository = root.join(".git").exists();
        let mut ignores = Self { rules: Vec::new(), repository };
        for name in [".gitignore", ".git/info/exclude"] {
            if let Ok(text) = std::fs::read_to_string(root.join(name)) {
                ignores.add_lines(text.lines());
            }
        }
        ignores.add_lines(extra.split(','));
        ignores
    }

    /// The patterns and nothing read from disk. What a test builds, and what a folder with no
    /// ignore file of any kind gets.
    pub fn from_patterns(patterns: &str) -> Self {
        let mut ignores = Self::default();
        ignores.add_lines(patterns.split(','));
        ignores
    }

    /// Whether this root is a repository, and therefore whether [`BUILD_OUTPUT`] is consulted.
    pub fn is_repository(&self) -> bool {
        self.repository
    }

    /// Nothing is ignored and this is not a repository, so only the three names apply.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    fn add_lines<'a>(&mut self, lines: impl Iterator<Item = &'a str>) {
        for line in lines {
            if let Some(rule) = read_rule(line) {
                self.rules.push(rule);
            }
        }
    }

    /// Whether `relative` — a path below the root, written with `/` separators — is left out.
    ///
    /// **The last rule that matches wins**, which is `gitignore(5)`'s own rule and is what makes a
    /// `!` line mean anything.
    pub fn ignores(&self, relative: &str, is_directory: bool) -> bool {
        let mut ignored = false;
        for rule in &self.rules {
            if rule.matches(relative, is_directory) {
                ignored = !rule.negated;
            }
        }
        ignored
    }

    /// The same question asked with a full path, for a caller that has one.
    pub fn ignores_path(&self, root: &Path, path: &Path, is_directory: bool) -> bool {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.is_empty() {
            return false;
        }
        self.ignores(&relative, is_directory)
    }

    /// Whether a folder called `name` at `relative` is walked into at all.
    ///
    /// The two questions are one function because they are always asked together, and because the
    /// fallback is only reached when the project has said nothing: inside a repository the three
    /// build folders are ignored **only if `.gitignore` says so**, which every repository that has
    /// one does, and outside one they are ignored because there is nothing else to go on.
    pub fn skips_folder(&self, relative: &str, name: &str) -> bool {
        if self.ignores(relative, true) {
            return true;
        }
        !self.repository && BUILD_OUTPUT.contains(&name)
    }
}

/// One line into a rule, or nothing for a blank line or a comment.
fn read_rule(line: &str) -> Option<Rule> {
    // Trailing whitespace is not part of a pattern unless it was escaped, which nobody does; leading
    // whitespace is not either, though git is stricter about that than this is.
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (negated, line) = match line.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    // **Read before the slashes come off**, because a leading `/` is the whole of what anchoring
    // means and stripping it first loses it — which is what the first draft of this did.
    let leading = line.starts_with('/');
    let (directory_only, line) = match line.strip_suffix('/') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let line = line.trim_start_matches('/');
    if line.is_empty() {
        return None;
    }
    // A pattern with a `/` anywhere but at its end is measured from the root. `doc/frotz` is
    // `<root>/doc/frotz`; `frotz` is any component called `frotz`, at any depth.
    let anchored = leading || line.contains('/');
    Some(Rule { pattern: line.to_owned(), negated, directory_only, anchored })
}

impl Rule {
    /// Whether this rule matches `relative`, a `/`-separated path below the root.
    ///
    /// A rule is tried against the path itself **and against every folder above it**, because
    /// ignoring `target/` has to ignore everything under it and the walk asks about files as well as
    /// folders. Every one of those prefixes is a folder by construction, which is what lets a
    /// directory-only rule leave out a *file* three levels down without ever matching the file.
    fn matches(&self, relative: &str, is_directory: bool) -> bool {
        let last = relative.len();
        prefixes(relative).any(|part| {
            // Only the whole path can be a file; each prefix names a folder it is inside.
            let part_is_directory = part.len() != last || is_directory;
            if self.directory_only && !part_is_directory {
                return false;
            }
            if self.anchored {
                glob_path(&self.pattern, part)
            } else {
                glob(&self.pattern, part.rsplit('/').next().unwrap_or(part))
            }
        })
    }
}

/// `a/b/c` as `a`, `a/b`, `a/b/c` — the path and every folder above it.
fn prefixes(relative: &str) -> impl Iterator<Item = &str> {
    relative
        .char_indices()
        .filter(|(_, character)| *character == '/')
        .map(|(at, _)| &relative[..at])
        .chain(std::iter::once(relative))
}

/// A whole path against a pattern that has folders in it.
///
/// **Segment by segment rather than as one string**, and that is the shape rather than an
/// implementation detail: `*` and `**` differ only in whether they may cross a `/`, so the clearest
/// reading is one where the `/` has already been taken out. `**` is "any number of segments,
/// including none", every other segment is [`glob`], and neither can then cross a boundary by
/// accident. The first draft matched over the flat string with one backtracking star and could not
/// answer `logs/**/*.log` at all, because a failed later `*` has to resume at the earlier `**` and
/// one saved position cannot hold both.
fn glob_path(pattern: &str, text: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let text: Vec<&str> = text.split('/').collect();
    segments(&pattern, &text)
}

fn segments(pattern: &[&str], text: &[&str]) -> bool {
    match pattern.first() {
        // Both ran out together.
        None => text.is_empty(),
        Some(&"**") => {
            // Any number of segments, including none. Tried shortest first.
            (0..=text.len()).any(|eaten| segments(&pattern[1..], &text[eaten..]))
        }
        Some(head) => match text.first() {
            Some(first) if glob(head, first) => segments(&pattern[1..], &text[1..]),
            _ => false,
        },
    }
}

/// `gitignore(5)`'s glob, over **one path segment**, so no `/` is involved and `*` cannot cross one.
///
/// Written as a backtracking matcher over bytes rather than with a regular expression crate, because
/// it is forty lines and the alternative is a dependency in a crate that has been careful about them.
fn glob(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let (mut p, mut t) = (0usize, 0usize);
    // Where to resume from if the match fails later: the `*` last passed, and how much of the text it
    // had consumed. One position is enough here, which is the reason the `/` is taken out first: two
    // stars in one segment are interchangeable, so the later one can always eat what the earlier one
    // would have.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        let matched = p < pattern.len()
            && match pattern[p] {
                b'*' => {
                    star = Some((p, t));
                    p += 1;
                    continue;
                }
                b'?' => {
                    p += 1;
                    t += 1;
                    continue;
                }
                b'[' => match class(&pattern[p..], text[t]) {
                    Some((length, true)) => {
                        p += length;
                        t += 1;
                        continue;
                    }
                    _ => false,
                },
                byte => byte == text[t],
            };
        if matched {
            p += 1;
            t += 1;
            continue;
        }
        // No match here. Back up to the last `*` and let it eat one more byte.
        match star {
            Some((at, consumed)) if consumed < text.len() => {
                star = Some((at, consumed + 1));
                t = consumed + 1;
                p = at + 1;
            }
            _ => return false,
        }
    }
    // The text is done; whatever is left of the pattern has to be stars.
    pattern[p.min(pattern.len())..].iter().all(|byte| *byte == b'*')
}

/// `[abc]`, `[a-z]` and `[!abc]`, answering how long the class was and whether `byte` is in it.
fn class(pattern: &[u8], byte: u8) -> Option<(usize, bool)> {
    let mut at = 1;
    let negated = matches!(pattern.get(at), Some(b'!') | Some(b'^'));
    if negated {
        at += 1;
    }
    let mut found = false;
    let mut first = true;
    while at < pattern.len() {
        if pattern[at] == b']' && !first {
            return Some((at + 1, found != negated));
        }
        first = false;
        // `a-z`, when there is a `-` with something after it that is not the closing bracket.
        if pattern.get(at + 1) == Some(&b'-') && pattern.get(at + 2).is_some_and(|next| *next != b']')
        {
            if byte >= pattern[at] && byte <= pattern[at + 2] {
                found = true;
            }
            at += 3;
        } else {
            if byte == pattern[at] {
                found = true;
            }
            at += 1;
        }
    }
    // An unclosed `[` is a literal bracket, which is what git does with it.
    None
}

/// The root of the repository `start` is in, or `start` itself when it is not in one.
///
/// The ignore rules belong to the repository rather than to whichever folder somebody happened to
/// open, so opening `crates/unluminous-core` in a checkout still honours the checkout's own file.
pub fn repository_root(start: &Path) -> PathBuf {
    let mut folder = start;
    loop {
        if folder.join(".git").exists() {
            return folder.to_path_buf();
        }
        match folder.parent() {
            Some(parent) => folder = parent,
            None => return start.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignores(patterns: &str) -> Ignores {
        Ignores::from_patterns(patterns)
    }

    #[test]
    fn a_plain_name_matches_at_any_depth_and_takes_everything_under_it() {
        let rules = ignores("target");
        assert!(rules.ignores("target", true));
        assert!(rules.ignores("target/debug/thing.rlib", false));
        assert!(rules.ignores("crates/app/target/debug/thing.rlib", false));
        assert!(!rules.ignores("crates/target-practice.rs", false));
    }

    /// The measurement in the ticket: a gitignored scratch copy of the whole project.
    #[test]
    fn the_scratch_folder_that_polluted_go_to_definition_is_left_out() {
        let rules = ignores("_agent_output/");
        assert!(rules.ignores(
            "_agent_output/task-1701-git-root-refresh/release-worktree/crates/quill-core/src/layout.rs",
            false
        ));
        assert!(!rules.ignores("crates/unluminous-core/src/layout.rs", false));
    }

    #[test]
    fn a_trailing_slash_means_a_directory_and_not_a_file_of_the_same_name() {
        let rules = ignores("build/");
        assert!(rules.ignores("build", true));
        assert!(!rules.ignores("build", false), "a *file* called build is kept");
        assert!(rules.ignores("build/output.js", false), "and everything under the folder goes");
    }

    #[test]
    fn a_leading_slash_anchors_the_pattern_to_the_root() {
        let rules = ignores("/build");
        assert!(rules.ignores("build", true));
        assert!(!rules.ignores("crates/app/build", true), "not this one, it is not at the root");
    }

    #[test]
    fn a_pattern_with_a_slash_in_it_is_measured_from_the_root() {
        let rules = ignores("doc/frotz");
        assert!(rules.ignores("doc/frotz", true));
        assert!(!rules.ignores("a/doc/frotz", true));
    }

    #[test]
    fn a_star_does_not_cross_a_folder_and_two_stars_do() {
        let one = ignores("*.log");
        assert!(one.ignores("debug.log", false));
        assert!(one.ignores("logs/debug.log", false), "unanchored, so it is tried on the name");

        let anchored = ignores("logs/*.log");
        assert!(anchored.ignores("logs/debug.log", false));
        assert!(!anchored.ignores("logs/nested/debug.log", false), "a star stops at the slash");

        let deep = ignores("logs/**/*.log");
        assert!(deep.ignores("logs/nested/deeper/debug.log", false));
    }

    #[test]
    fn a_question_mark_is_one_character_and_a_class_is_a_set_of_them() {
        assert!(ignores("file?.txt").ignores("file1.txt", false));
        assert!(!ignores("file?.txt").ignores("file10.txt", false));
        assert!(ignores("file[0-9].txt").ignores("file7.txt", false));
        assert!(!ignores("file[0-9].txt").ignores("filea.txt", false));
        assert!(ignores("file[!0-9].txt").ignores("filea.txt", false));
    }

    /// The last rule that matches wins, which is what makes `!` mean anything.
    #[test]
    fn a_negation_after_a_rule_brings_the_file_back() {
        let rules = ignores("*.log, !keep.log");
        assert!(rules.ignores("debug.log", false));
        assert!(!rules.ignores("keep.log", false));
    }

    #[test]
    fn comments_and_blank_lines_are_not_rules() {
        let mut rules = Ignores::default();
        rules.add_lines(["# a comment", "", "   ", "target"].into_iter());
        assert!(rules.ignores("target", true));
        assert!(!rules.ignores("a-comment", false));
    }

    /// Outside a repository the three build folders are still what is skipped, which is what they
    /// always were. Inside one, the project's own file decides.
    #[test]
    fn the_three_build_folders_are_the_fallback_outside_a_repository() {
        let outside = Ignores::default();
        assert!(outside.skips_folder("node_modules", "node_modules"));
        assert!(!outside.skips_folder("dist", "dist"));

        let inside = Ignores { rules: Vec::new(), repository: true };
        assert!(
            !inside.skips_folder("node_modules", "node_modules"),
            "a repository that does not ignore node_modules means it"
        );
    }

    #[test]
    fn the_rules_are_read_off_the_disk_and_the_settings_line_is_read_after_them() {
        let folder = std::env::temp_dir().join("unluminous-ignore-test");
        std::fs::create_dir_all(folder.join(".git/info")).expect("make the test folders");
        std::fs::write(folder.join(".gitignore"), "*.log\nbuild/\n").expect("write .gitignore");
        std::fs::write(folder.join(".git/info/exclude"), "scratch/\n").expect("write exclude");
        let rules = Ignores::read(&folder, "vendor/, !keep.log");
        assert!(rules.is_repository(), "the .git folder makes it one");
        assert!(rules.ignores("debug.log", false));
        assert!(rules.ignores("build/out.js", false));
        assert!(rules.ignores("scratch/notes.txt", false), "info/exclude is read too");
        assert!(rules.ignores("vendor/lib.rs", false), "and the setting");
        assert!(!rules.ignores("keep.log", false), "the setting is read last, so it can win");
    }

    #[test]
    fn the_repository_root_is_found_from_a_folder_inside_it() {
        let folder = std::env::temp_dir().join("unluminous-ignore-root-test");
        let inner = folder.join("crates/core/src");
        std::fs::create_dir_all(&inner).expect("make the folders");
        std::fs::create_dir_all(folder.join(".git")).expect("make the git folder");
        assert_eq!(repository_root(&inner), folder);
    }
}
