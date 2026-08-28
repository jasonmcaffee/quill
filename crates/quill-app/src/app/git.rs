//! The window's git state: the repository, the thread that runs the commands, and what it last said.
//!
//! Nothing here draws and nothing here runs a git command. The commands run on
//! [`quill_git::Worker`]'s thread and the drawing is in `components::git_panel` and
//! `components::git_dialogs`; this is what sits between them, holding the last answer so that the
//! window has something to draw between one command and the next.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quill_git::worker::{Reply, Request, Snapshot};
use quill_git::{Commit, Repository, Worker};

use crate::app::files::OpenFiles;
use crate::components::git_panel::CommitPanel;
use crate::components::git_dialogs::GitDialogs;
use crate::components::gutter::{BlameRow, Change};

/// How many commits the history window reads. A repository can hold a hundred thousand; a window
/// shows a few dozen and scrolling past two hundred is not how anyone looks for a commit.
pub const HISTORY_LIMIT: usize = 200;

/// Everything the window knows about the repository it is in.
pub struct GitState {
    pub repository: Repository,
    worker: Worker,
    /// What the last refresh read.
    pub snapshot: Snapshot,
    pub panel: CommitPanel,
    pub dialogs: GitDialogs,
    pub history: Vec<Commit>,
    /// The messages of the last few commits, for the panel's clock button.
    pub recent_messages: Vec<String>,
    /// What git last said, for the status bar.
    pub message: Option<String>,
    /// Set once the first read has come back.
    read: bool,
}

impl GitState {
    /// Start working on the repository `folder` is in, if it is in one.
    pub fn open(folder: &Path, waker: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let repository = Repository::discover(folder)?;
        Some(Self::from_repository(repository, waker))
    }

    /// Start working on an already discovered repository.
    pub fn from_repository(repository: Repository, waker: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut worker = Worker::start(repository.clone(), waker);
        worker.send(Request::Refresh);
        Self {
            repository,
            worker,
            snapshot: Snapshot::default(),
            panel: CommitPanel::default(),
            dialogs: GitDialogs::default(),
            history: Vec::new(),
            recent_messages: Vec::new(),
            message: None,
            read: false,
        }
    }

    /// Ask the thread for something.
    pub fn send(&mut self, request: Request) {
        self.message = Some(format!("{}\u{2026}", request.label()));
        self.worker.send(request);
    }

    /// What is running, for the status bar.
    pub fn running(&self) -> Option<&str> {
        self.worker.running()
    }

    /// Whether the worker still owes the window its first or latest answer.
    pub fn is_busy(&self) -> bool {
        !self.read || self.worker.running().is_some()
    }

    /// A path as git spells it, relative to the root.
    pub fn relative(&self, path: &Path) -> Option<String> {
        self.repository.relative(path)
    }

    /// Take everything the thread has answered and put it where the window will draw it.
    ///
    /// Returns true when something needs laying out again, which is only ever the blame column and
    /// the change bars, because those are the only replies that change what the editing area shows.
    pub fn take_replies(&mut self, files: &mut OpenFiles) -> bool {
        let mut redraw = false;
        for reply in self.worker.poll() {
            match reply {
                Reply::Snapshot(snapshot) => {
                    self.snapshot = *snapshot;
                    self.read = true;
                    // A commit that has just been made is not in the panel's message any more.
                    if let Some(label) = self.snapshot.in_progress {
                        self.message = Some(format!("{label} \u{2014} finish it or abandon it from the Git menu"));
                    }
                }
                Reply::Blame(path, blame) => {
                    let rows: Vec<BlameRow> = blame
                        .lines
                        .into_iter()
                        .map(|line| BlameRow {
                            date: line.date,
                            author: shorten(&line.author),
                            commit: line.commit,
                            age: line.age,
                            summary: line.summary,
                        })
                        .collect();
                    if let Some(index) = files.index_of(&path) {
                        if let Some(file) = files.get_mut(index) {
                            file.blame = Some(rows);
                        }
                        redraw = true;
                    }
                    self.message = None;
                }
                Reply::ChangedLines(path, changes) => {
                    let changes: Vec<(usize, Change)> = changes
                        .into_iter()
                        .map(|(line, kind)| {
                            (
                                line,
                                match kind {
                                    quill_git::LineChange::Added => Change::Added,
                                    quill_git::LineChange::Modified => Change::Modified,
                                },
                            )
                        })
                        .collect();
                    if let Some(index) = files.index_of(&path) {
                        if let Some(file) = files.get_mut(index) {
                            file.line_changes = changes;
                        }
                        redraw = true;
                    }
                    self.message = None;
                }
                Reply::Log(commits) => {
                    self.recent_messages = commits
                        .iter()
                        .take(20)
                        .map(|commit| {
                            if commit.body.trim().is_empty() {
                                commit.subject.clone()
                            } else {
                                format!("{}\n\n{}", commit.subject, commit.body.trim())
                            }
                        })
                        .collect();
                    self.history = commits;
                    self.message = None;
                }
                Reply::Text { title, body } => {
                    self.dialogs.open =
                        Some(crate::components::git_dialogs::Dialog::Text { title, body });
                    self.message = None;
                }
                Reply::Done { label, outcome } => {
                    // Git's own message, always. A rejected push and a merge conflict both explain
                    // themselves better than anything Quill could say about them.
                    let said = outcome.summary();
                    self.message = Some(if outcome.ok {
                        if said.is_empty() { format!("{label}: done") } else { said }
                    } else {
                        format!("{label} failed: {said}")
                    });
                    self.worker.send(Request::Refresh);
                    self.worker.send(Request::Log { path: None, limit: HISTORY_LIMIT });
                    // What git says about a file has changed, so anything annotated is annotated
                    // against a version that has gone.
                    files.forget_git();
                    redraw = true;
                }
                Reply::Cloned { folder, outcome } => {
                    self.message = Some(if outcome.ok {
                        format!("Cloned into {}", folder.display())
                    } else {
                        format!("Clone failed: {}", outcome.summary())
                    });
                    if outcome.ok {
                        crate::services::launcher::open_window(&folder);
                    }
                }
            }
        }
        redraw
    }

    /// Ask for the blame and the change bars of the file that is showing, when it is in this
    /// repository and has not been asked for already.
    pub fn refresh_file(&mut self, path: Option<&Path>, want_blame: bool, want_changes: bool) {
        let Some(path) = path else {
            return;
        };
        if self.relative(path).is_none() {
            return;
        }
        if want_blame {
            self.send(Request::Blame(path.to_path_buf()));
        }
        if want_changes {
            self.send(Request::ChangedLines(path.to_path_buf()));
        }
    }

    /// What the status bar says about the repository, once there is anything to say.
    ///
    /// `None` until the first read comes back. Reading happens on a thread, so for the first few
    /// frames there is nothing to report, and an empty status has no branch in it — which the label
    /// spells `detached HEAD`. Announcing that in the gap is alarming, wrong, and the first thing
    /// anybody sees when they open a project.
    pub fn status_label(&self) -> Option<String> {
        if !self.read {
            return None;
        }
        let mut label = self.snapshot.status.branch_label();
        if let Some(what) = self.snapshot.in_progress {
            label = format!("{label} \u{00B7} {what}");
        }
        let changed = self.snapshot.status.entries.len();
        if changed > 0 {
            label = format!("{label} \u{00B7} {changed} changed");
        }
        Some(label)
    }

    /// What git thinks of a file in the explorer, so the row can be tinted by it.
    pub fn state_of(&self, path: &Path) -> Option<quill_git::State> {
        let relative = self.relative(path)?;
        let entry = self.snapshot.status.entry(&relative)?;
        Some(if entry.index == quill_git::State::Unchanged { entry.worktree } else { entry.index })
    }
}

/// How the discovered repository root relates to the project Quill opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootRelation {
    Project,
    Ancestor,
}

impl RootRelation {
    /// The stable spelling used by command-line JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Ancestor => "ancestor",
        }
    }
}

/// Compare two folders after resolving aliases where the file system permits it.
fn same_folder(left: &Path, right: &Path) -> bool {
    left.canonicalize().unwrap_or_else(|_| left.to_path_buf())
        == right.canonicalize().unwrap_or_else(|_| right.to_path_buf())
}

/// Say whether a repository is the project itself or one of its ancestors.
pub fn root_relation(repository: &Path, project: &Path) -> RootRelation {
    if same_folder(repository, project) { RootRelation::Project } else { RootRelation::Ancestor }
}

/// Whether the project itself has acquired a normal or worktree git marker.
pub fn has_direct_marker(project: &Path) -> bool {
    project.join(".git").exists()
}

/// An author's first name, which is what fits in a blame column.
///
/// `Jason McAffee` becomes `Jason`, and an address becomes the part in front of the at sign, because
/// a commit made by a robot often has no name at all. A single word is left alone.
fn shorten(author: &str) -> String {
    let author = author.trim();
    if let Some((before, _)) = author.split_once('@') {
        return before.to_owned();
    }
    author.split_whitespace().next().unwrap_or(author).to_owned()
}

/// Where to look for a repository when a window opens: the folder the explorer is showing.
pub fn repository_for(folder: &Path) -> Option<PathBuf> {
    Repository::discover(folder).map(|repository| repository.root().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The relation distinguishes the project root from a repository above it.
    #[test]
    fn a_repository_root_says_whether_it_is_the_project_or_an_ancestor() {
        let project = Path::new("project");
        assert_eq!(root_relation(project, project), RootRelation::Project);
        assert_eq!(root_relation(Path::new("."), project), RootRelation::Ancestor);
    }

    #[test]
    fn a_blame_column_shows_a_first_name() {
        assert_eq!(shorten("Jason McAffee"), "Jason");
        assert_eq!(shorten("Jason"), "Jason");
        assert_eq!(shorten("  Kim Lee  "), "Kim");
        // A commit with an address where a name should be, which a robot often makes.
        assert_eq!(shorten("bot@example.com"), "bot");
        assert_eq!(shorten(""), "");
    }
}
