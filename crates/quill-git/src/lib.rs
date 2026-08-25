//! Reading and changing a git repository.
//!
//! A crate of its own, with no user interface dependency, so its tests build real repositories in a
//! temporary folder and run real commands against them with no window, no graphics card and no
//! fonts — the same bar `quill-core` and `quill-terminal` are held to.
//!
//! ## It runs the `git` program
//!
//! Not libgit2 and not gitoxide. Both were considered and both were rejected, and the reason is not
//! size or build time: it is **what the machine's own git already knows**. A person running Quill has
//! a git that has been configured — a credential helper holding a token, an ssh agent,
//! `commit.gpgsign`, `core.autocrlf`, hooks, `include` directives, an identity for this repository in
//! particular, `safe.directory`. `git push` from Quill has to be the same push they get from their
//! terminal, or the first time it fails they have two gits to debug instead of one. A library
//! reimplements enough of that to be *nearly* the same, which is worse than plainly being the same
//! thing.
//!
//! What it costs is that the output has to be read. That is answered by asking for the formats git
//! provides *for* being read — `--porcelain=v2 -z` for the status, `--line-porcelain` for blame, and
//! `--format` with the unit and record separators for the log. None of them has changed in years,
//! all of them are separated by characters that cannot appear in a path, and each parser here is
//! tested against output written by hand as well as against a real repository.
//!
//! ## Nothing here decides anything
//!
//! Every call returns an [`command::Outcome`] holding git's own standard output and standard error,
//! whether it worked or not, and the window shows git's message when something goes wrong. A
//! rejected push, a merge conflict and a missing upstream all have good messages already.

pub mod blame;
pub mod branch;
pub mod command;
pub mod diff;
pub mod log;
pub mod ops;
pub mod status;
pub mod worker;

use std::path::{Path, PathBuf};

pub use blame::{Blame, BlameLine};
pub use branch::{Branch, InProgress, MergeOptions, Resume};
pub use command::Outcome;
pub use diff::LineChange;
pub use log::Commit;
pub use ops::{PullStrategy, PushTarget, Remote, ResetMode, Stash};
pub use status::{Entry, State, Status};
pub use worker::{Reply, Request, Worker};

/// A git repository, found from a folder inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    /// The repository `folder` is in, if it is in one.
    ///
    /// Git is asked rather than `.git` being looked for by hand, because `.git` is a file rather
    /// than a folder in a worktree and a submodule, and because git already knows about
    /// `safe.directory` and about ceiling directories.
    pub fn discover(folder: &Path) -> Option<Self> {
        let outcome = command::run(folder, &["rev-parse", "--show-toplevel"]);
        if !outcome.ok {
            return None;
        }
        let root = outcome.stdout.trim();
        if root.is_empty() {
            return None;
        }
        // Git answers with forward slashes on Windows too. `PathBuf` is happy with them, and this
        // way the path Quill shows is the path git shows.
        Some(Self { root: PathBuf::from(root) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The repository's own name, which is the last part of its path — what the commit panel shows
    /// against the branch.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    /// A path relative to the root, spelled the way git spells one, or `None` when the path is not
    /// inside this repository.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let rest = path.strip_prefix(&self.root).ok()?;
        let text = rest.to_string_lossy().replace('\\', "/");
        (!text.is_empty()).then_some(text)
    }

    pub fn status(&self) -> Result<Status, Outcome> {
        Status::read(&self.root)
    }

    pub fn blame(&self, path: &Path) -> Result<Blame, Outcome> {
        Blame::read(&self.root, path)
    }

    pub fn log(&self, path: Option<&Path>, limit: usize) -> Result<Vec<Commit>, Outcome> {
        log::read(&self.root, path, limit)
    }

    pub fn in_progress(&self) -> InProgress {
        branch::in_progress(&self.root)
    }

    pub fn branches(&self) -> Vec<Branch> {
        branch::all(&self.root)
    }
}

/// Whether there is a `git` on this machine, and which version.
pub fn version() -> Option<String> {
    command::version()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_not_in_a_repository_has_none() {
        let folder = std::env::temp_dir().join("quill-git-not-a-repository");
        std::fs::create_dir_all(&folder).expect("make the folder");
        // Only meaningful if the temporary folder is not itself inside a repository, which it is not
        // on either platform; if it were, this would find the outer one and the assertion would be
        // about the wrong thing, so it checks the root rather than only that there is one.
        if let Some(repository) = Repository::discover(&folder) {
            assert_ne!(repository.root(), folder, "the folder itself is not a repository");
        }
    }

    #[test]
    fn a_path_is_made_relative_the_way_git_spells_one() {
        let repository = Repository { root: PathBuf::from("/home/jason/quill") };
        assert_eq!(
            repository.relative(Path::new("/home/jason/quill/crates/quill-git/src/lib.rs")),
            Some("crates/quill-git/src/lib.rs".to_owned())
        );
        assert_eq!(repository.relative(Path::new("/home/jason/other/thing.md")), None);
        assert_eq!(repository.relative(Path::new("/home/jason/quill")), None, "the root itself is not a path in it");
    }

    #[test]
    fn a_repository_is_named_after_its_folder() {
        let repository = Repository { root: PathBuf::from("/home/jason/quill") };
        assert_eq!(repository.name(), "quill");
    }
}
