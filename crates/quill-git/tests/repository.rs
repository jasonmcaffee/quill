//! Every git operation, run against real repositories.
//!
//! A parser can be tested against output written by hand, and each one in this crate is. What that
//! cannot show is whether the command line was right — whether `Rollback` really discards the
//! change, whether ticking an untracked file really stages it, whether a push to a remote really
//! arrives. So each test here builds a repository in a temporary folder, does the thing, and then
//! asks **git** what happened rather than asking Quill.
//!
//! Each test gets a folder of its own, named after it, so they can run at the same time and a
//! failure leaves the repository behind to be looked at.
//!
//! Every repository is built with its identity and its settings named on the command line, so a test
//! does not depend on the `.gitconfig` of whoever is running it. Signing in particular: a machine
//! with `commit.gpgsign` on would otherwise fail every commit here for a reason that has nothing to
//! do with Quill.

use std::path::{Path, PathBuf};

use quill_git::branch::{self, MergeOptions, Resume};
use quill_git::command::run;
use quill_git::ops::{self, PullStrategy, PushTarget, ResetMode};
use quill_git::{diff, log, Repository, State, Status};

/// A repository with one commit in it, at a folder named after the test.
fn repository(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join("quill-git-tests").join(name);
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the folder");
    init(&root);
    write(&root, "readme.md", "# a repository\n");
    commit_all(&root, "the first commit");
    root
}

/// `git init`, with everything a test depends on set here rather than read from the machine.
fn init(root: &Path) {
    assert!(run(root, &["init", "--initial-branch=main"]).ok, "git init");
    for (name, value) in [
        ("user.name", "Quill Test"),
        ("user.email", "test@quill.invalid"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        // A test must not depend on the machine's line ending setting, or a file written with a
        // newline comes back with a carriage return in it and every comparison fails.
        ("core.autocrlf", "false"),
    ] {
        assert!(run(root, &["config", name, value]).ok, "git config {name}");
    }
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("make the folder");
    }
    std::fs::write(full, text).expect("write the file");
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).expect("read the file")
}

fn commit_all(root: &Path, message: &str) {
    assert!(run(root, &["add", "-A"]).ok, "git add -A");
    let outcome = ops::commit(root, message, false);
    assert!(outcome.ok, "git commit: {}", outcome.message());
}

/// Commit with a date named on the command line.
///
/// Two commits made one after the other land in the same second, so a test about what is older
/// than what has to say which is which rather than relying on the clock.
fn commit_all_dated(root: &Path, message: &str, date: &str) {
    assert!(run(root, &["add", "-A"]).ok, "git add -A");
    let outcome = run(root, &["commit", "--date", date, "-m", message]);
    assert!(outcome.ok, "git commit: {}", outcome.message());
}

fn status(root: &Path) -> Status {
    Status::read(root).expect("git status")
}

#[test]
fn a_repository_is_found_from_a_folder_inside_it() {
    let root = repository("discover");
    std::fs::create_dir_all(root.join("deep/inside")).expect("make the folders");
    let found = Repository::discover(&root.join("deep/inside")).expect("it is in a repository");
    // Compared through `canonicalize`, because the temporary folder is behind a short path on
    // Windows and git answers with the long one.
    assert_eq!(
        found.root().canonicalize().expect("canonicalize"),
        root.canonicalize().expect("canonicalize")
    );
    assert_eq!(found.name(), "discover");
}

#[test]
fn a_clean_tree_has_nothing_in_it_and_a_dirty_one_lists_what_changed() {
    let root = repository("status");
    assert!(status(&root).is_clean());

    write(&root, "readme.md", "# changed\n");
    write(&root, "scratch.txt", "not tracked\n");
    let status = status(&root);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.entry("readme.md").expect("readme").worktree, State::Modified);
    assert!(status.entry("scratch.txt").expect("scratch").untracked());
    assert_eq!(status.untracked_count(), 1);
    assert_eq!(status.staged_count(), 0, "nothing is staged until it is added");
}

#[test]
fn staging_and_unstaging_move_a_file_between_the_two_sides() {
    let root = repository("staging");
    write(&root, "readme.md", "# changed\n");
    assert!(ops::add(&root, &["readme.md"]).ok);
    assert!(status(&root).entry("readme.md").expect("readme").staged());
    assert!(ops::unstage(&root, &["readme.md"]).ok);
    assert!(!status(&root).entry("readme.md").expect("readme").staged());
    assert_eq!(read(&root, "readme.md"), "# changed\n", "unstaging leaves what is on disk alone");
}

#[test]
fn an_untracked_file_can_be_staged_and_taken_back_out_again() {
    // This is the `Unversioned Files` group in the commit panel: ticking one has to stage it, and
    // unticking has to work even though there is nothing in HEAD to restore from.
    let root = repository("untracked");
    write(&root, "scratch.txt", "new\n");
    assert!(ops::add(&root, &["scratch.txt"]).ok);
    assert!(status(&root).entry("scratch.txt").expect("scratch").staged());
    assert!(ops::unstage(&root, &["scratch.txt"]).ok);
    assert!(status(&root).entry("scratch.txt").expect("scratch").untracked());
    assert!(root.join("scratch.txt").is_file(), "the file itself is still there");
}

#[test]
fn committing_writes_a_commit_and_amending_changes_the_one_before() {
    let root = repository("commit");
    write(&root, "notes.md", "one\n");
    assert!(ops::add(&root, &["notes.md"]).ok);
    assert!(ops::commit(&root, "add notes", false).ok);

    let commits = log::read(&root, None, 10).expect("log");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "add notes");
    assert_eq!(commits[0].author, "Quill Test");

    write(&root, "notes.md", "one\ntwo\n");
    assert!(ops::add(&root, &["notes.md"]).ok);
    assert!(ops::commit(&root, "add notes, with more", true).ok);
    let commits = log::read(&root, None, 10).expect("log");
    assert_eq!(commits.len(), 2, "amending changes a commit rather than adding one");
    assert_eq!(commits[0].subject, "add notes, with more");
}

#[test]
fn rollback_discards_what_was_not_committed() {
    let root = repository("rollback");
    write(&root, "readme.md", "# spoiled\n");
    assert!(ops::rollback(&root, &["readme.md"]).ok);
    assert_eq!(read(&root, "readme.md"), "# a repository\n");
    assert!(status(&root).is_clean());
}

#[test]
fn rollback_also_takes_back_something_that_was_staged() {
    let root = repository("rollback-staged");
    write(&root, "readme.md", "# spoiled\n");
    assert!(ops::add(&root, &["readme.md"]).ok);
    assert!(ops::rollback(&root, &["readme.md"]).ok);
    assert!(status(&root).is_clean(), "rollback takes both sides, which is what IntelliJ's does");
}

#[test]
fn a_branch_can_be_started_switched_to_and_deleted() {
    let root = repository("branches");
    assert!(branch::create(&root, "feature").ok);
    assert_eq!(branch::current(&root).as_deref(), Some("feature"));
    let names: Vec<String> = branch::all(&root).into_iter().map(|found| found.name).collect();
    assert!(names.contains(&"feature".to_owned()) && names.contains(&"main".to_owned()));
    assert!(branch::all(&root).iter().any(|found| found.name == "feature" && found.current));

    assert!(branch::switch(&root, "main").ok);
    assert_eq!(branch::current(&root).as_deref(), Some("main"));
    assert!(branch::delete(&root, "feature", false).ok);
    let names: Vec<String> = branch::all(&root).into_iter().map(|found| found.name).collect();
    assert!(!names.contains(&"feature".to_owned()));
}

#[test]
fn a_merge_that_fits_together_brings_the_other_branch_in() {
    let root = repository("merge");
    assert!(branch::create(&root, "feature").ok);
    write(&root, "feature.md", "from the branch\n");
    commit_all(&root, "work on the branch");
    assert!(branch::switch(&root, "main").ok);
    assert!(!root.join("feature.md").exists());

    let outcome = branch::merge(&root, "feature", MergeOptions::default());
    assert!(outcome.ok, "merge: {}", outcome.message());
    assert_eq!(read(&root, "feature.md"), "from the branch\n");
    assert_eq!(branch::in_progress(&root), branch::InProgress::Nothing);
}

#[test]
fn a_merge_that_conflicts_says_so_and_can_be_abandoned() {
    let root = repository("conflict");
    write(&root, "shared.md", "the original\n");
    commit_all(&root, "add shared");

    assert!(branch::create(&root, "feature").ok);
    write(&root, "shared.md", "the branch's version\n");
    commit_all(&root, "change it on the branch");

    assert!(branch::switch(&root, "main").ok);
    write(&root, "shared.md", "main's version\n");
    commit_all(&root, "change it on main");

    let outcome = branch::merge(&root, "feature", MergeOptions::default());
    assert!(!outcome.ok, "a conflict is a failure, and git says why");
    assert!(
        outcome.message().to_lowercase().contains("conflict"),
        "git's own message is what the window shows: {}",
        outcome.message()
    );
    assert_eq!(branch::in_progress(&root), branch::InProgress::Merging);
    let status = status(&root);
    assert_eq!(status.conflicts().len(), 1);
    assert_eq!(status.conflicts()[0].path, "shared.md");
    // The file on disk holds the markers, which is a file holding text and therefore something
    // Quill can open. There is no three way merge editor and this is why one is not needed to
    // finish.
    assert!(read(&root, "shared.md").contains("<<<<<<<"));

    assert!(branch::resume_merge(&root, Resume::Abort).ok);
    assert_eq!(branch::in_progress(&root), branch::InProgress::Nothing);
    assert_eq!(read(&root, "shared.md"), "main's version\n");
}

#[test]
fn a_conflict_can_also_be_settled_and_the_merge_finished() {
    let root = repository("conflict-resolved");
    write(&root, "shared.md", "the original\n");
    commit_all(&root, "add shared");
    assert!(branch::create(&root, "feature").ok);
    write(&root, "shared.md", "the branch's version\n");
    commit_all(&root, "branch");
    assert!(branch::switch(&root, "main").ok);
    write(&root, "shared.md", "main's version\n");
    commit_all(&root, "main");
    assert!(!branch::merge(&root, "feature", MergeOptions::default()).ok);

    // Settling a conflict is editing the file and staging it, which is what Quill does with it.
    write(&root, "shared.md", "both versions, by hand\n");
    assert!(ops::add(&root, &["shared.md"]).ok);
    let outcome = branch::resume_merge(&root, Resume::Continue);
    assert!(outcome.ok, "continue: {}", outcome.message());
    assert_eq!(branch::in_progress(&root), branch::InProgress::Nothing);
    assert!(status(&root).is_clean());
}

#[test]
fn a_rebase_puts_the_commits_on_top_of_the_other_branch() {
    let root = repository("rebase");
    assert!(branch::create(&root, "feature").ok);
    write(&root, "feature.md", "from the branch\n");
    commit_all(&root, "work on the branch");
    assert!(branch::switch(&root, "main").ok);
    write(&root, "main.md", "on main\n");
    commit_all(&root, "work on main");
    assert!(branch::switch(&root, "feature").ok);

    let outcome = branch::rebase(&root, "main");
    assert!(outcome.ok, "rebase: {}", outcome.message());
    let subjects: Vec<String> =
        log::read(&root, None, 10).expect("log").into_iter().map(|c| c.subject).collect();
    assert_eq!(subjects[0], "work on the branch");
    assert_eq!(subjects[1], "work on main", "the branch's commit is now on top of main's");
}

#[test]
fn reset_moves_the_branch_and_each_mode_does_what_it_says() {
    let root = repository("reset");
    write(&root, "notes.md", "one\n");
    commit_all(&root, "second");
    assert_eq!(log::read(&root, None, 10).expect("log").len(), 2);

    // Soft: the commit goes, what was in it stays staged.
    assert!(ops::reset(&root, "HEAD~1", ResetMode::Soft).ok);
    assert_eq!(log::read(&root, None, 10).expect("log").len(), 1);
    assert!(status(&root).entry("notes.md").expect("notes").staged());

    // Mixed: staged becomes not staged, the file is still there.
    assert!(ops::reset(&root, "HEAD", ResetMode::Mixed).ok);
    assert!(status(&root).entry("notes.md").expect("notes").untracked());
    assert!(root.join("notes.md").is_file());

    // Hard: everything git was tracking goes back. An untracked file is not git's to remove.
    write(&root, "readme.md", "# spoiled\n");
    assert!(ops::reset(&root, "HEAD", ResetMode::Hard).ok);
    assert_eq!(read(&root, "readme.md"), "# a repository\n");
}

#[test]
fn a_stash_puts_the_changes_away_and_unstashing_brings_them_back() {
    let root = repository("stash");
    write(&root, "readme.md", "# in progress\n");
    let outcome = ops::stash(&root, "half done", false);
    assert!(outcome.ok, "stash: {}", outcome.message());
    assert_eq!(read(&root, "readme.md"), "# a repository\n", "the tree is clean again");

    let stashes = ops::stashes(&root);
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("half done"));
    assert_eq!(stashes[0].name, "stash@{0}");

    assert!(ops::unstash(&root, "stash@{0}", true).ok);
    assert_eq!(read(&root, "readme.md"), "# in progress\n");
    assert!(ops::stashes(&root).is_empty(), "pop takes the entry off the list");
}

#[test]
fn an_untracked_file_is_only_stashed_when_it_is_asked_for() {
    let root = repository("stash-untracked");
    write(&root, "scratch.txt", "new\n");
    assert!(ops::stash(&root, "without it", false).ok);
    assert!(root.join("scratch.txt").is_file(), "an untracked file is left alone by default");
    // Nothing was stashed, because there was nothing else to stash.
    assert!(ops::stashes(&root).is_empty());

    assert!(ops::stash(&root, "with it", true).ok);
    assert!(!root.join("scratch.txt").exists());
    assert!(ops::unstash(&root, "stash@{0}", true).ok);
    assert!(root.join("scratch.txt").is_file());
}

#[test]
fn a_push_to_another_folder_really_arrives_and_a_pull_brings_it_back() {
    // A folder on disk is a perfectly good remote, which is what makes it possible to test a push
    // without a network or an account.
    let root = repository("push");
    let bare = std::env::temp_dir().join("quill-git-tests").join("push-remote.git");
    std::fs::remove_dir_all(&bare).ok();
    std::fs::create_dir_all(&bare).expect("make the remote");
    assert!(run(&bare, &["init", "--bare", "--initial-branch=main"]).ok, "git init --bare");

    assert!(ops::add_remote(&root, "origin", &bare.display().to_string()).ok);
    assert_eq!(ops::remotes(&root).len(), 1, "listed once, not twice, though git prints it twice");

    let target = PushTarget {
        remote: "origin".to_owned(),
        branch: "main".to_owned(),
        set_upstream: true,
        force: false,
        tags: false,
    };
    let outcome = ops::push(&root, &target);
    assert!(outcome.ok, "push: {}", outcome.message());

    // The commit really is on the other side.
    let there = run(&bare, &["log", "--format=%s", "-n1"]);
    assert_eq!(there.stdout.trim(), "the first commit");

    // Clone it somewhere else, commit there, and pull that back.
    let elsewhere = std::env::temp_dir().join("quill-git-tests").join("push-clone");
    std::fs::remove_dir_all(&elsewhere).ok();
    std::fs::create_dir_all(&elsewhere).expect("make the folder");
    let (outcome, cloned) = ops::clone(&elsewhere, &bare.display().to_string());
    assert!(outcome.ok, "clone: {}", outcome.message());
    assert!(cloned.join("readme.md").is_file(), "the clone landed in {}", cloned.display());
    for (name, value) in [("user.name", "Someone Else"), ("user.email", "else@quill.invalid"), ("commit.gpgsign", "false")] {
        assert!(run(&cloned, &["config", name, value]).ok);
    }
    write(&cloned, "from-elsewhere.md", "written on the other side\n");
    commit_all(&cloned, "a commit from elsewhere");
    assert!(ops::push(&cloned, &target).ok);

    // Back in the first repository: fetch sees it, pull brings it in.
    assert!(ops::fetch(&root).ok);
    let status = Status::read(&root).expect("status");
    assert_eq!(status.behind, 1, "one commit is waiting on the other side");
    assert!(ops::pull(&root, "origin", "main", PullStrategy::Merge).ok);
    assert_eq!(read(&root, "from-elsewhere.md"), "written on the other side\n");
    assert_eq!(Status::read(&root).expect("status").behind, 0);
}

#[test]
fn blame_says_who_wrote_each_line_and_ranks_the_commits_by_age() {
    let root = repository("blame");
    write(&root, "code.ts", "const one = 1;\n");
    commit_all_dated(&root, "the first line", "2026-04-25T10:00:00+00:00");
    // A second author, so the test proves the name comes from the commit rather than from the
    // machine's own configuration.
    assert!(run(&root, &["config", "user.name", "Someone Else"]).ok);
    write(&root, "code.ts", "const one = 1;\nconst two = 2;\n");
    commit_all_dated(&root, "the second line", "2026-04-26T10:00:00+00:00");

    let blame = Repository::discover(&root).expect("a repository").blame(Path::new("code.ts")).expect("blame");
    assert_eq!(blame.lines.len(), 2);
    assert_eq!(blame.lines[0].author, "Quill Test");
    assert_eq!(blame.lines[1].author, "Someone Else");
    assert_eq!(blame.lines[0].summary, "the first line");
    assert_eq!(blame.lines[0].age, 0.0, "the older commit is at one end of the colour");
    assert_eq!(blame.lines[1].age, 1.0, "and the newer one at the other");
    assert!(blame.lines[0].date.contains('/'), "the date is formatted, not a number of seconds");
}

#[test]
fn the_history_of_one_file_holds_only_the_commits_that_touched_it() {
    let root = repository("history");
    write(&root, "one.md", "one\n");
    commit_all(&root, "add one");
    write(&root, "two.md", "two\n");
    commit_all(&root, "add two");

    let all = log::read(&root, None, 10).expect("log");
    assert_eq!(all.len(), 3);
    let one = log::read(&root, Some(Path::new("one.md")), 10).expect("log");
    let subjects: Vec<String> = one.into_iter().map(|commit| commit.subject).collect();
    assert_eq!(subjects, vec!["add one"]);

    let messages = log::recent_messages(&root, 3);
    assert_eq!(messages[0], "add two", "the commit panel offers the newest message first");
}

#[test]
fn a_repository_with_no_commits_in_it_has_an_empty_history_rather_than_an_error() {
    let root = std::env::temp_dir().join("quill-git-tests").join("empty");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the folder");
    init(&root);
    assert!(log::read(&root, None, 10).expect("an empty history, not a failure").is_empty());
}

#[test]
fn the_gutter_is_told_which_lines_differ_from_the_version_git_has() {
    let root = repository("changed-lines");
    write(&root, "code.ts", "one\ntwo\nthree\nfour\n");
    commit_all(&root, "four lines");

    write(&root, "code.ts", "one\nCHANGED\nthree\nfour\nfive\n");
    let changes = diff::changed_lines(&root, &root.join("code.ts"));
    // Line two changed, line five is new. Counted from zero, the way the layout counts paragraphs.
    assert_eq!(changes, vec![(1, diff::LineChange::Modified), (4, diff::LineChange::Added)]);

    // A file git has never seen is new all the way down.
    write(&root, "brand-new.ts", "a\nb\nc\n");
    let changes = diff::changed_lines(&root, &root.join("brand-new.ts"));
    assert_eq!(changes.len(), 3);
    assert!(changes.iter().all(|(_, kind)| *kind == diff::LineChange::Added));
}

#[test]
fn a_diff_is_git_s_own_and_a_failure_carries_git_s_own_message() {
    let root = repository("diff");
    write(&root, "readme.md", "# changed\n");
    let outcome = diff::of_path(&root, Path::new("readme.md"), false, None);
    assert!(outcome.ok);
    assert!(outcome.stdout.contains("-# a repository"));
    assert!(outcome.stdout.contains("+# changed"));

    // Something that cannot work: git explains, and the explanation is what is handed back rather
    // than a message of Quill's own invention.
    let outcome = branch::switch(&root, "no-such-branch");
    assert!(!outcome.ok);
    assert!(
        outcome.message().contains("no-such-branch"),
        "git's message names what was wrong: {}",
        outcome.message()
    );
}

#[test]
fn a_tag_is_written_and_a_rename_is_reported_with_the_name_it_had() {
    let root = repository("tag-and-rename");
    assert!(ops::tag(&root, "v1.0.0").ok);
    let tags = run(&root, &["tag", "--list"]);
    assert_eq!(tags.stdout.trim(), "v1.0.0");

    std::fs::rename(root.join("readme.md"), root.join("README.markdown")).expect("rename");
    assert!(ops::add(&root, &["."]).ok);
    let status = status(&root);
    let renamed = status
        .entries
        .iter()
        .find(|entry| entry.index == State::Renamed)
        .expect("git notices a rename once both sides are staged");
    assert_eq!(renamed.path, "README.markdown");
    assert_eq!(renamed.from.as_deref(), Some("readme.md"));
}

#[test]
fn a_path_with_a_space_in_it_survives_the_whole_round_trip() {
    // The reason every command that could print a path asks for `-z`.
    let root = repository("spaces");
    write(&root, "my notes.md", "one\n");
    let listed = status(&root);
    assert!(listed.entry("my notes.md").is_some(), "found: {:?}", listed.entries);
    assert!(ops::add(&root, &["my notes.md"]).ok);
    assert!(status(&root).entry("my notes.md").expect("notes").staged());
    assert!(ops::commit(&root, "a file with a space in its name", false).ok);
    assert!(status(&root).is_clean());
}
