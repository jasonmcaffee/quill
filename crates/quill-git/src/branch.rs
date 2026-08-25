//! Branches, and the three things done with another branch: switching to it, merging it in, and
//! rebasing onto it.
//!
//! Branches are read with `for-each-ref` rather than `git branch`, because `git branch` is a
//! porcelain command whose output is meant for a person: it marks the current branch with an
//! asterisk, indents, and abbreviates. `for-each-ref` takes a format, so the fields come back
//! exactly as asked for.

use std::path::Path;

use crate::command::{run, Outcome};

/// One branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// `main`, or `origin/main` for one on a remote.
    pub name: String,
    /// True when it lives on a remote rather than here.
    pub remote: bool,
    /// True when it is the one checked out.
    pub current: bool,
    /// What it tracks, for a local branch that tracks something.
    pub upstream: Option<String>,
}

/// Every branch, local ones first and then the remotes', each group by name.
pub fn all(folder: &Path) -> Vec<Branch> {
    let mut branches = read(folder, "refs/heads", false);
    branches.extend(read(folder, "refs/remotes", true));
    branches
}

/// The branch that is checked out, if HEAD is on one.
pub fn current(folder: &Path) -> Option<String> {
    let outcome = run(folder, &["symbolic-ref", "--short", "-q", "HEAD"]);
    outcome.ok.then(|| outcome.stdout.trim().to_owned()).filter(|name| !name.is_empty())
}

fn read(folder: &Path, namespace: &str, remote: bool) -> Vec<Branch> {
    let outcome = run(
        folder,
        &[
            "for-each-ref",
            "--format=%(refname:short)\u{1f}%(HEAD)\u{1f}%(upstream:short)",
            "--sort=refname",
            namespace,
        ],
    );
    if !outcome.ok {
        return Vec::new();
    }
    outcome
        .stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\u{1f}').collect();
            let name = fields.first()?.trim();
            if name.is_empty() {
                return None;
            }
            // `origin/HEAD` is a pointer at the remote's default branch, not a branch of its own,
            // and offering it to be checked out would put HEAD in a strange place.
            if remote && name.ends_with("/HEAD") {
                return None;
            }
            let upstream = fields.get(2).map(|value| value.trim()).filter(|value| !value.is_empty());
            Some(Branch {
                name: name.to_owned(),
                remote,
                current: fields.get(1).is_some_and(|marker| marker.trim() == "*"),
                upstream: upstream.map(str::to_owned),
            })
        })
        .collect()
}

/// Check out an existing branch.
///
/// `git switch` rather than `git checkout`, because `checkout` does two unrelated jobs — moving
/// between branches and restoring files — and picking the wrong one has thrown away work for
/// everyone who has ever used git. `switch` only moves between branches.
///
/// Checking out a remote branch by its remote name starts a local branch that tracks it, which is
/// what a person clicking `origin/feature` means and is what `git switch` does on its own.
pub fn switch(folder: &Path, name: &str) -> Outcome {
    run(folder, &["switch", name])
}

/// Start a branch here and move to it.
pub fn create(folder: &Path, name: &str) -> Outcome {
    run(folder, &["switch", "-c", name])
}

/// Delete a branch. `force` is the difference between `-d`, which refuses to delete a branch holding
/// commits that are nowhere else, and `-D`, which does it anyway.
pub fn delete(folder: &Path, name: &str, force: bool) -> Outcome {
    run(folder, &["branch", if force { "-D" } else { "-d" }, name])
}

/// How a merge is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeOptions {
    /// Make a merge commit even when the branch could simply be moved forward.
    pub no_fast_forward: bool,
    /// Bring the changes in without committing them, so they can be committed as one.
    pub squash: bool,
}

pub fn merge(folder: &Path, name: &str, options: MergeOptions) -> Outcome {
    let mut arguments: Vec<&str> = vec!["merge"];
    if options.no_fast_forward {
        arguments.push("--no-ff");
    }
    if options.squash {
        arguments.push("--squash");
    }
    arguments.push(name);
    run(folder, &arguments)
}

pub fn rebase(folder: &Path, name: &str) -> Outcome {
    run(folder, &["rebase", name])
}

/// What to do about a merge or a rebase that stopped on a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    Continue,
    Abort,
}

pub fn resume_merge(folder: &Path, what: Resume) -> Outcome {
    match what {
        // `--continue` takes no arguments at all, not even `--no-edit`, which is a thing git says
        // plainly and which a test caught. It does open an editor for the merge message, and what
        // stops that hanging is `GIT_EDITOR=true` in `command::run`, which is set for exactly this.
        Resume::Continue => run(folder, &["merge", "--continue"]),
        Resume::Abort => run(folder, &["merge", "--abort"]),
    }
}

pub fn resume_rebase(folder: &Path, what: Resume) -> Outcome {
    match what {
        Resume::Continue => run(folder, &["rebase", "--continue"]),
        Resume::Abort => run(folder, &["rebase", "--abort"]),
    }
}

/// What the repository is in the middle of, if anything.
///
/// Read from the files git leaves in its own directory while an operation is unfinished, which is
/// how git itself knows. The status bar says so, and the Git menu grows `Continue` and `Abort`,
/// because an editor that hides a half-finished merge is an editor you cannot finish one in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgress {
    Nothing,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
}

impl InProgress {
    pub fn label(self) -> Option<&'static str> {
        match self {
            InProgress::Nothing => None,
            InProgress::Merging => Some("Merging"),
            InProgress::Rebasing => Some("Rebasing"),
            InProgress::CherryPicking => Some("Cherry-picking"),
            InProgress::Reverting => Some("Reverting"),
        }
    }
}

/// What `git` directory this repository keeps its state in, which is not always `.git`: in a
/// worktree it is a file pointing elsewhere, and `rev-parse` is the only thing that knows.
pub fn git_directory(folder: &Path) -> Option<std::path::PathBuf> {
    let outcome = run(folder, &["rev-parse", "--absolute-git-dir"]);
    outcome.ok.then(|| std::path::PathBuf::from(outcome.stdout.trim()))
}

pub fn in_progress(folder: &Path) -> InProgress {
    let Some(git) = git_directory(folder) else {
        return InProgress::Nothing;
    };
    if git.join("MERGE_HEAD").exists() {
        InProgress::Merging
    } else if git.join("rebase-merge").exists() || git.join("rebase-apply").exists() {
        InProgress::Rebasing
    } else if git.join("CHERRY_PICK_HEAD").exists() {
        InProgress::CherryPicking
    } else if git.join("REVERT_HEAD").exists() {
        InProgress::Reverting
    } else {
        InProgress::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_something_unfinished_has_a_label() {
        assert_eq!(InProgress::Nothing.label(), None);
        assert_eq!(InProgress::Merging.label(), Some("Merging"));
        assert_eq!(InProgress::Rebasing.label(), Some("Rebasing"));
    }

    #[test]
    fn merge_options_build_the_command_line_they_say_they_do() {
        // The options are a small struct rather than three booleans in a row, so that a caller
        // cannot pass them in the wrong order.
        let plain = MergeOptions::default();
        assert!(!plain.no_fast_forward && !plain.squash);
        let both = MergeOptions { no_fast_forward: true, squash: true };
        assert!(both.no_fast_forward && both.squash);
    }
}
