//! Everything that changes a repository, one function per entry on the Git menu.
//!
//! Each one is thin on purpose: it builds a command line and hands back what git said. The reason it
//! is worth having them written down at all, rather than the window building command lines, is that
//! several of these are easy to assume wrongly, and the exact command each entry runs is then in one
//! place where it can be read and argued with.
//!
//! Two are worth naming here because they are the ones people are surprised by:
//!
//! - **Rollback** discards uncommitted changes. It is `git restore --source=HEAD --staged
//!   --worktree`, which is what the reference editor's Rollback does, and there is no undo for it. The window
//!   asks first.
//! - **Push with force** is `--force-with-lease`, never a bare `--force`. The difference is that
//!   `--force-with-lease` refuses when the remote has moved since you last fetched, which is exactly
//!   the case where a bare force throws away somebody else's work.

use std::ffi::OsString;
use std::path::Path;

use crate::command::{run, Outcome};

/// Stage paths, which is what a tick in the commit panel does.
///
/// `git add` rather than `git stage`: they are the same command and `add` is the one every
/// explanation of git in the world uses. It works on an untracked file as well, which is what makes
/// ticking a row in `Unversioned Files` stage it.
pub fn add(folder: &Path, paths: &[&str]) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["add".into(), "--".into()];
    arguments.extend(paths.iter().map(OsString::from));
    run(folder, &arguments)
}

/// Unstage paths, leaving what is on disk alone. Unticking a row in the commit panel.
pub fn unstage(folder: &Path, paths: &[&str]) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["restore".into(), "--staged".into(), "--".into()];
    arguments.extend(paths.iter().map(OsString::from));
    let outcome = run(folder, &arguments);
    if outcome.ok {
        return outcome;
    }
    // In a repository with no commits yet there is no HEAD to restore from, and `git rm --cached`
    // is the only way to take a file back out of the index. Without this, the very first commit's
    // tick boxes are one-way.
    let mut arguments: Vec<OsString> = vec!["rm".into(), "--cached".into(), "-r".into(), "--".into()];
    arguments.extend(paths.iter().map(OsString::from));
    run(folder, &arguments)
}

/// Throw away uncommitted changes to paths, in the index and on disk.
///
/// This is Rollback, and it cannot be undone: the changes are not in a commit and not in a stash, so
/// after this they are nowhere. The window confirms first and says so.
pub fn rollback(folder: &Path, paths: &[&str]) -> Outcome {
    let mut arguments: Vec<OsString> =
        vec!["restore".into(), "--source=HEAD".into(), "--staged".into(), "--worktree".into(), "--".into()];
    arguments.extend(paths.iter().map(OsString::from));
    run(folder, &arguments)
}

/// Commit what is staged.
///
/// Hooks run, because a hook is something the repository's owner asked for and a commit from Unluminate
/// should be the same commit as one from the terminal.
pub fn commit(folder: &Path, message: &str, amend: bool) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["commit".into(), "-m".into(), message.into()];
    if amend {
        arguments.push("--amend".into());
    }
    run(folder, &arguments)
}

/// Where a push is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub remote: String,
    pub branch: String,
    /// True when the branch has no upstream yet, so the push has to set one.
    pub set_upstream: bool,
    /// `--force-with-lease`, never a bare `--force`.
    pub force: bool,
    pub tags: bool,
}

pub fn push(folder: &Path, target: &PushTarget) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["push".into()];
    if target.set_upstream {
        arguments.push("--set-upstream".into());
    }
    if target.force {
        arguments.push("--force-with-lease".into());
    }
    if target.tags {
        arguments.push("--tags".into());
    }
    arguments.push(target.remote.as_str().into());
    arguments.push(target.branch.as_str().into());
    run(folder, &arguments)
}

/// How a pull brings the other side's commits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullStrategy {
    Merge,
    Rebase,
}

pub fn pull(folder: &Path, remote: &str, branch: &str, strategy: PullStrategy) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["pull".into()];
    arguments.push(match strategy {
        PullStrategy::Merge => "--no-rebase".into(),
        PullStrategy::Rebase => "--rebase".into(),
    });
    arguments.push(remote.into());
    arguments.push(branch.into());
    run(folder, &arguments)
}

/// Fetch every remote, and forget remote branches that have been deleted on the other side.
pub fn fetch(folder: &Path) -> Outcome {
    run(folder, &["fetch", "--all", "--prune"])
}

/// The four ways of moving the branch, which are the four the reference editor's Reset dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// The commits go, the changes stay staged.
    Soft,
    /// The commits go, the changes stay on disk and are not staged.
    Mixed,
    /// The commits go and so do the changes. There is no undo.
    Hard,
    /// The commits go, and changes that were never committed are kept.
    Keep,
}

impl ResetMode {
    pub const ALL: [ResetMode; 4] = [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard, ResetMode::Keep];

    pub fn name(self) -> &'static str {
        match self {
            ResetMode::Soft => "Soft",
            ResetMode::Mixed => "Mixed",
            ResetMode::Hard => "Hard",
            ResetMode::Keep => "Keep",
        }
    }

    /// One line saying what it does, which is what the dialog shows under the name.
    pub fn description(self) -> &'static str {
        match self {
            ResetMode::Soft => "The commits go. What was in them stays staged, ready to commit again.",
            ResetMode::Mixed => "The commits go. What was in them stays on disk, not staged.",
            ResetMode::Hard => "The commits go and so does everything in them. This cannot be undone.",
            ResetMode::Keep => "The commits go. Changes you had not committed are kept.",
        }
    }

    fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        }
    }
}

pub fn reset(folder: &Path, revision: &str, mode: ResetMode) -> Outcome {
    run(folder, &["reset", mode.flag(), revision])
}

/// Put the changes away under a message, so the working tree is clean.
pub fn stash(folder: &Path, message: &str, include_untracked: bool) -> Outcome {
    let mut arguments: Vec<OsString> = vec!["stash".into(), "push".into()];
    if include_untracked {
        arguments.push("--include-untracked".into());
    }
    if !message.trim().is_empty() {
        arguments.push("-m".into());
        arguments.push(message.into());
    }
    run(folder, &arguments)
}

/// One entry in the stash list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`, which is what every stash command takes.
    pub name: String,
    pub message: String,
}

pub fn stashes(folder: &Path) -> Vec<Stash> {
    let outcome = run(folder, &["stash", "list", "--format=%gd\u{1f}%s"]);
    if !outcome.ok {
        return Vec::new();
    }
    outcome
        .stdout
        .lines()
        .filter_map(|line| line.split_once('\u{1f}'))
        .map(|(name, message)| Stash { name: name.trim().to_owned(), message: message.trim().to_owned() })
        .collect()
}

/// Put a stash back. `drop` is the difference between `git stash pop` and `git stash apply`.
pub fn unstash(folder: &Path, name: &str, drop: bool) -> Outcome {
    run(folder, &["stash", if drop { "pop" } else { "apply" }, name])
}

pub fn drop_stash(folder: &Path, name: &str) -> Outcome {
    run(folder, &["stash", "drop", name])
}

pub fn tag(folder: &Path, name: &str) -> Outcome {
    run(folder, &["tag", name])
}

/// One remote, and where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

pub fn remotes(folder: &Path) -> Vec<Remote> {
    let outcome = run(folder, &["remote", "-v"]);
    if !outcome.ok {
        return Vec::new();
    }
    let mut found: Vec<Remote> = Vec::new();
    for line in outcome.stdout.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(url)) = (fields.next(), fields.next()) else {
            continue;
        };
        // `git remote -v` lists each remote twice, once for fetch and once for push.
        if found.iter().any(|remote| remote.name == name) {
            continue;
        }
        found.push(Remote { name: name.to_owned(), url: url.to_owned() });
    }
    found
}

pub fn add_remote(folder: &Path, name: &str, url: &str) -> Outcome {
    run(folder, &["remote", "add", name, url])
}

pub fn set_remote_url(folder: &Path, name: &str, url: &str) -> Outcome {
    run(folder, &["remote", "set-url", name, url])
}

pub fn remove_remote(folder: &Path, name: &str) -> Outcome {
    run(folder, &["remote", "remove", name])
}

/// Clone `url` into a folder under `parent`, and say where it ended up.
///
/// The folder is named after the repository, the way `git clone` names it, so the caller can open
/// the result without having to ask git where it went.
pub fn clone(parent: &Path, url: &str) -> (Outcome, std::path::PathBuf) {
    let name = clone_folder_name(url);
    let target = parent.join(&name);
    let outcome = run(parent, &["clone", url, &name]);
    (outcome, target)
}

/// The folder `git clone` would put a repository in: the last part of the address, without
/// `.git`.
///
/// The separators are all three that can appear: a forward slash in an address, a colon after
/// the host in an ssh address, and a backslash in a path on Windows. Leaving the backslash out
/// is not a small mistake. Cloning from a path on Windows then produced the whole path as the
/// folder's *name*, and git dutifully cloned it to the top of the drive instead of where it was
/// asked to, which is exactly what a test cloning between two temporary folders found.
pub fn clone_folder_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next().unwrap_or(trimmed);
    let name = last.trim_end_matches(".git");
    if name.is_empty() {
        "repository".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_lands_in_a_folder_named_after_the_repository() {
        assert_eq!(clone_folder_name("https://github.com/jasonmcaffee/unluminate.git"), "unluminate");
        assert_eq!(clone_folder_name("https://github.com/jasonmcaffee/unluminate"), "unluminate");
        assert_eq!(clone_folder_name("git@github.com:jasonmcaffee/unluminate.git"), "unluminate");
        assert_eq!(clone_folder_name("https://example.com/thing/"), "thing");
        assert_eq!(clone_folder_name(""), "repository");
        // A path on Windows, which is a perfectly good thing to clone from and which used to
        // come back as the whole path rather than as a name.
        assert_eq!(clone_folder_name("C:\\repos\\thing.git"), "thing");
        assert_eq!(clone_folder_name(r"\\server\share\thing"), "thing");
    }

    #[test]
    fn every_reset_mode_says_what_it_does_and_only_one_of_them_is_final() {
        for mode in ResetMode::ALL {
            assert!(!mode.description().is_empty(), "{} should say what it does", mode.name());
        }
        assert!(ResetMode::Hard.description().contains("cannot be undone"));
        assert!(!ResetMode::Soft.description().contains("cannot be undone"));
    }
}
