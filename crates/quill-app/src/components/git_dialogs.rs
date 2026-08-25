//! The dialogs behind the Git menu: push, pull, merge, rebase, reset, branches, history, remotes,
//! and the panel that shows a diff or a commit.
//!
//! One file, because they are eight variations on the same modal and splitting them across eight
//! files would hide how alike they are. Each is a short function built from `components::modal`, and
//! none of them runs a git command: each returns what was asked for and `QuillApp::run_action` sends
//! it to the worker thread.

use egui::{Pos2, Rect, Vec2};
use quill_git::{Branch, Commit, PullStrategy, PushTarget, Remote, ResetMode, Status};

use crate::components::modal;
use crate::theme::color;

/// Which dialog is open. Only one can be, because they are all modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    Push,
    Pull,
    /// Merge or rebase: the two differ only in what the button says.
    Merge { rebase: bool },
    Reset,
    Branches,
    History,
    Remotes,
    /// A diff, a commit, or anything else git printed.
    Text { title: String, body: String },
}

/// What the dialogs are showing and what has been typed into them.
#[derive(Debug, Default)]
pub struct GitDialogs {
    pub open: Option<Dialog>,
    /// The remote and branch a push or a pull is aimed at.
    pub remote: String,
    pub branch: String,
    pub force: bool,
    pub tags: bool,
    pub rebase_on_pull: bool,
    pub no_fast_forward: bool,
    pub squash: bool,
    /// The branch, tag or commit a merge, rebase or reset is about.
    pub target: String,
    pub reset_mode: usize,
    /// A new remote's name and address.
    pub remote_name: String,
    pub remote_url: String,
}

impl GitDialogs {
    /// Open a dialog, filling in what it needs from the repository as it stands.
    pub fn open(&mut self, dialog: Dialog, status: &Status, remotes: &[Remote]) {
        if self.remote.is_empty() {
            self.remote =
                remotes.first().map(|remote| remote.name.clone()).unwrap_or_else(|| "origin".to_owned());
        }
        self.branch = status.branch.clone().unwrap_or_default();
        self.target.clear();
        self.open = Some(dialog);
    }

    pub fn close(&mut self) {
        self.open = None;
    }
}

/// What a dialog asked for.
#[derive(Debug, Default, PartialEq)]
pub struct DialogOutcome {
    pub close: bool,
    pub push: Option<PushTarget>,
    pub pull: Option<(String, String, PullStrategy)>,
    pub merge: Option<(String, quill_git::MergeOptions)>,
    pub rebase: Option<String>,
    pub reset: Option<(String, ResetMode)>,
    pub switch: Option<String>,
    pub delete_branch: Option<String>,
    pub show_commit: Option<String>,
    pub add_remote: Option<(String, String)>,
    pub remove_remote: Option<String>,
}

/// Draw whichever dialog is open.
pub fn show(
    ctx: &egui::Context,
    dialogs: &mut GitDialogs,
    status: &Status,
    branches: &[Branch],
    remotes: &[Remote],
    history: &[Commit],
) -> DialogOutcome {
    let mut outcome = DialogOutcome::default();
    let Some(dialog) = dialogs.open.clone() else {
        return outcome;
    };
    let (title, width, height) = match &dialog {
        Dialog::Push => ("Push Commits", 560.0, 340.0),
        Dialog::Pull => ("Pull Changes", 560.0, 320.0),
        Dialog::Merge { rebase: false } => ("Merge into the current branch", 560.0, 420.0),
        Dialog::Merge { rebase: true } => ("Rebase onto", 560.0, 420.0),
        Dialog::Reset => ("Reset HEAD", 560.0, 430.0),
        Dialog::Branches => ("Branches", 560.0, 520.0),
        Dialog::History => ("History", 860.0, 560.0),
        Dialog::Remotes => ("Remotes", 620.0, 440.0),
        Dialog::Text { title, .. } => (title.as_str(), 900.0, 620.0),
    };
    let (_, closed) = modal::show(ctx, "quill-git-dialog", width, height, |ui, area| {
        if modal::header(ui, area, title) {
            outcome.close = true;
        }
        let body = modal::body(area);
        match &dialog {
            Dialog::Push => push(ui, area, body, dialogs, status, remotes, &mut outcome),
            Dialog::Pull => pull(ui, area, body, dialogs, remotes, &mut outcome),
            Dialog::Merge { rebase } => {
                merge(ui, area, body, dialogs, branches, *rebase, &mut outcome)
            }
            Dialog::Reset => reset(ui, area, body, dialogs, history, &mut outcome),
            Dialog::Branches => branch_list(ui, area, body, branches, &mut outcome),
            Dialog::History => history_list(ui, area, body, history, &mut outcome),
            Dialog::Remotes => remote_list(ui, area, body, dialogs, remotes, &mut outcome),
            Dialog::Text { body: text, .. } => {
                modal::monospaced(ui, body, "git-text", text);
                if modal::footer(ui, area, &[("CLOSE", true)]).is_some() {
                    outcome.close = true;
                }
            }
        }
    });
    if closed {
        outcome.close = true;
    }
    if outcome.close {
        dialogs.close();
    }
    outcome
}

/// `Push...`: where it is going, and the two things that can be asked of it.
fn push(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    dialogs: &mut GitDialogs,
    status: &Status,
    remotes: &[Remote],
    outcome: &mut DialogOutcome,
) {
    let mut pen = modal::note(
        ui,
        body,
        body.top(),
        &format!(
            "{} commit(s) to push. Forcing uses --force-with-lease, which refuses when the remote has moved since you last fetched.",
            status.ahead
        ),
    );
    pen = named_field(ui, body, pen, "Remote", &mut dialogs.remote);
    pen = named_field(ui, body, pen, "Branch", &mut dialogs.branch);
    let list: Vec<String> = remotes.iter().map(|remote| format!("{}  {}", remote.name, remote.url)).collect();
    pen = modal::note(ui, body, pen + 4.0, &list.join("\n"));
    modal::check(ui, row_at(body, pen), "Force with lease", &mut dialogs.force);
    modal::check(ui, row_at(body, pen + 26.0), "Push tags", &mut dialogs.tags);

    let ready = !dialogs.remote.trim().is_empty() && !dialogs.branch.trim().is_empty();
    match modal::footer(ui, area, &[("CANCEL", true), ("PUSH", ready)]) {
        Some(0) => outcome.close = true,
        Some(1) => {
            outcome.push = Some(PushTarget {
                remote: dialogs.remote.trim().to_owned(),
                branch: dialogs.branch.trim().to_owned(),
                // With no upstream yet the push has to set one, or the next one has to be told
                // where to go all over again.
                set_upstream: status.upstream.is_none(),
                force: dialogs.force,
                tags: dialogs.tags,
            })
        }
        _ => {}
    }
}

/// `Pull...`: where from, and whether it merges or rebases.
fn pull(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    dialogs: &mut GitDialogs,
    remotes: &[Remote],
    outcome: &mut DialogOutcome,
) {
    let mut pen = modal::note(
        ui,
        body,
        body.top(),
        "Bring the other side's commits in. Rebasing replays your commits on top of theirs instead of making a merge commit.",
    );
    pen = named_field(ui, body, pen, "Remote", &mut dialogs.remote);
    pen = named_field(ui, body, pen, "Branch", &mut dialogs.branch);
    let list: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
    pen = modal::note(ui, body, pen + 4.0, &format!("Remotes: {}", list.join(", ")));
    modal::check(ui, row_at(body, pen), "Rebase instead of merging", &mut dialogs.rebase_on_pull);

    let ready = !dialogs.remote.trim().is_empty() && !dialogs.branch.trim().is_empty();
    match modal::footer(ui, area, &[("CANCEL", true), ("PULL", ready)]) {
        Some(0) => outcome.close = true,
        Some(1) => {
            outcome.pull = Some((
                dialogs.remote.trim().to_owned(),
                dialogs.branch.trim().to_owned(),
                if dialogs.rebase_on_pull { PullStrategy::Rebase } else { PullStrategy::Merge },
            ))
        }
        _ => {}
    }
}

/// `Merge...` and `Rebase...`, which differ only in the button and the two options.
fn merge(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    dialogs: &mut GitDialogs,
    branches: &[Branch],
    rebase: bool,
    outcome: &mut DialogOutcome,
) {
    let note = if rebase {
        "Replay the commits on this branch on top of the one chosen below."
    } else {
        "Bring the branch chosen below into the one that is checked out."
    };
    let mut pen = modal::note(ui, body, body.top(), note);
    let list = Rect::from_min_max(
        Pos2::new(body.left(), pen),
        Pos2::new(body.right(), body.bottom() - if rebase { 8.0 } else { 60.0 }),
    );
    chooser(ui, list, "merge-branches", branches, &mut dialogs.target);
    if !rebase {
        pen = list.bottom() + 6.0;
        modal::check(ui, row_at(body, pen), "Always make a merge commit (--no-ff)", &mut dialogs.no_fast_forward);
        modal::check(ui, row_at(body, pen + 26.0), "Bring the changes in without committing (--squash)", &mut dialogs.squash);
    }

    let ready = !dialogs.target.trim().is_empty();
    let word = if rebase { "REBASE" } else { "MERGE" };
    match modal::footer(ui, area, &[("CANCEL", true), (word, ready)]) {
        Some(0) => outcome.close = true,
        Some(1) if rebase => outcome.rebase = Some(dialogs.target.clone()),
        Some(1) => {
            outcome.merge = Some((
                dialogs.target.clone(),
                quill_git::MergeOptions {
                    no_fast_forward: dialogs.no_fast_forward,
                    squash: dialogs.squash,
                },
            ))
        }
        _ => {}
    }
}

/// `Reset HEAD...`: where to move the branch to, and which of the four modes.
fn reset(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    dialogs: &mut GitDialogs,
    history: &[Commit],
    outcome: &mut DialogOutcome,
) {
    let mut pen = named_field(ui, body, body.top(), "To commit", &mut dialogs.target);
    if dialogs.target.trim().is_empty() {
        pen = modal::note(ui, body, pen, "A commit, a branch, or something like HEAD~1.");
    }
    pen = modal::section(ui, body, pen, "Mode");
    for (index, mode) in ResetMode::ALL.into_iter().enumerate() {
        let row = row_at(body, pen);
        let chosen = dialogs.reset_mode == index;
        let response = ui.interact(row, ui.id().with(("reset-mode", index)), egui::Sense::click());
        if chosen {
            ui.painter().rect_filled(
                row.shrink2(Vec2::new(0.0, 1.0)),
                egui::CornerRadius::same(5),
                color::SELECTED_ROW,
            );
        }
        let tint = if chosen { color::TEXT_STRONG } else { color::TEXT_CONTROL };
        let x = modal::label(ui.painter(), row, row.left() + 10.0, mode.name(), tint, 12.5);
        modal::label(ui.painter(), row, x + 14.0, mode.description(), color::TEXT_FAINT, 11.0);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, chosen, mode.name())
        });
        if response.clicked() {
            dialogs.reset_mode = index;
        }
        pen += 30.0;
    }
    // The most recent commits, so a revision can be read rather than remembered.
    let recent: Vec<String> =
        history.iter().take(4).map(|commit| format!("{}  {}", commit.short, commit.subject)).collect();
    modal::note(ui, body, pen + 4.0, &recent.join("\n"));

    let ready = !dialogs.target.trim().is_empty();
    match modal::footer(ui, area, &[("CANCEL", true), ("RESET", ready)]) {
        Some(0) => outcome.close = true,
        Some(1) => {
            outcome.reset =
                Some((dialogs.target.trim().to_owned(), ResetMode::ALL[dialogs.reset_mode]))
        }
        _ => {}
    }
}

/// `Branches...`: the list, with the one that is checked out marked.
fn branch_list(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    branches: &[Branch],
    outcome: &mut DialogOutcome,
) {
    let list = Rect::from_min_max(body.min, Pos2::new(body.right(), body.bottom()));
    let mut chosen: Option<String> = None;
    let mut current = String::new();
    let inner = list.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    egui::ScrollArea::vertical().id_salt("branch-list").show(&mut child, |ui| {
        for branch in branches {
            if branch.current {
                current = branch.name.clone();
            }
            let name = branch.name.clone();
            let remote = branch.remote;
            let is_current = branch.current;
            let upstream = branch.upstream.clone().unwrap_or_default();
            let response = modal::row(ui, &branch.name, &branch.name, is_current, |painter, row| {
                let tint = if is_current { color::TEXT_STRONG } else { color::TEXT_CONTROL };
                let x = modal::label(painter, row, row.left() + 16.0, &name, tint, 12.5);
                let note = if is_current {
                    "checked out".to_owned()
                } else if remote {
                    "on a remote".to_owned()
                } else if !upstream.is_empty() {
                    format!("tracks {upstream}")
                } else {
                    String::new()
                };
                if !note.is_empty() {
                    modal::label(painter, row, x + 14.0, &note, color::TEXT_FAINT, 11.0);
                }
            });
            if response.clicked() {
                chosen = Some(branch.name.clone());
            }
        }
    });
    let picked = chosen.clone().unwrap_or_default();
    let can_act = !picked.is_empty() && picked != current;
    match modal::footer(ui, area, &[("DELETE", can_act), ("CHECK OUT", can_act)]) {
        Some(0) => outcome.delete_branch = chosen,
        Some(1) => outcome.switch = chosen,
        _ => {}
    }
}

/// `Show History`: the commits, newest first.
fn history_list(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    history: &[Commit],
    outcome: &mut DialogOutcome,
) {
    let inner = body.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    egui::ScrollArea::vertical().id_salt("git-history").show(&mut child, |ui| {
        for commit in history {
            let short = commit.short.clone();
            let subject = commit.subject.clone();
            let author = commit.author.clone();
            let date = commit.date.clone();
            let refs = commit.refs.clone();
            let response = modal::row(ui, &commit.hash, &commit.subject, false, |painter, row| {
                let mut x = modal::label(painter, row, row.left() + 12.0, &short, color::ACCENT, 11.5);
                if !refs.is_empty() {
                    x = modal::label(painter, row, x + 10.0, &refs, color::GIT_ADDED, 10.5);
                }
                x = modal::label(painter, row, x + 12.0, &subject, color::TEXT_CONTROL, 12.0);
                x = modal::label(painter, row, x + 16.0, &author, color::TEXT_DIM, 11.0);
                modal::label(painter, row, x + 10.0, &date, color::TEXT_FAINT, 11.0);
            });
            if response.clicked() {
                outcome.show_commit = Some(commit.hash.clone());
            }
        }
        if history.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  No commits yet.").size(11.5).color(color::TEXT_FAINT));
        }
    });
    if modal::footer(ui, area, &[("CLOSE", true)]).is_some() {
        outcome.close = true;
    }
}

/// `Manage Remotes...`: the list, and a name and address to add one.
fn remote_list(
    ui: &mut egui::Ui,
    area: Rect,
    body: Rect,
    dialogs: &mut GitDialogs,
    remotes: &[Remote],
    outcome: &mut DialogOutcome,
) {
    let list = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + 160.0));
    let mut chosen: Option<String> = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.set_clip_rect(list);
    egui::ScrollArea::vertical().id_salt("remote-list").show(&mut child, |ui| {
        for remote in remotes {
            let name = remote.name.clone();
            let url = remote.url.clone();
            let response = modal::row(ui, &remote.name, &remote.name, false, |painter, row| {
                let x = modal::label(painter, row, row.left() + 16.0, &name, color::TEXT_STRONG, 12.5);
                modal::label(painter, row, x + 16.0, &url, color::TEXT_FAINT, 11.0);
            });
            if response.clicked() {
                chosen = Some(remote.name.clone());
            }
        }
        if remotes.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  No remotes.").size(11.5).color(color::TEXT_FAINT));
        }
    });
    let mut pen = modal::section(ui, body, list.bottom() + 8.0, "Add a remote");
    pen = named_field(ui, body, pen, "Name", &mut dialogs.remote_name);
    named_field(ui, body, pen, "Address", &mut dialogs.remote_url);

    let can_add =
        !dialogs.remote_name.trim().is_empty() && !dialogs.remote_url.trim().is_empty();
    match modal::footer(ui, area, &[("REMOVE", chosen.is_some()), ("ADD", can_add)]) {
        Some(0) => outcome.remove_remote = chosen,
        Some(1) => {
            outcome.add_remote =
                Some((dialogs.remote_name.trim().to_owned(), dialogs.remote_url.trim().to_owned()))
        }
        _ => {}
    }
}

/// A list of branches to choose one from, which merge and rebase both need.
fn chooser(ui: &mut egui::Ui, area: Rect, id: &str, branches: &[Branch], target: &mut String) {
    ui.painter().rect(
        area,
        egui::CornerRadius::same(6),
        color::EXPLORER_FOOTER,
        egui::Stroke::new(1.0, color::DIVIDER),
        egui::StrokeKind::Inside,
    );
    let inner = area.shrink(4.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    egui::ScrollArea::vertical().id_salt(id).show(&mut child, |ui| {
        for branch in branches.iter().filter(|branch| !branch.current) {
            let chosen = *target == branch.name;
            let name = branch.name.clone();
            let response = modal::row(ui, &branch.name, &branch.name, chosen, |painter, row| {
                let tint = if chosen { color::TEXT_STRONG } else { color::TEXT_CONTROL };
                modal::label(painter, row, row.left() + 16.0, &name, tint, 12.5);
            });
            if response.clicked() {
                *target = branch.name.clone();
            }
        }
    });
}

/// A label at the left and a field beside it, which most of these dialogs are made of.
fn named_field(ui: &mut egui::Ui, body: Rect, top: f32, name: &str, value: &mut String) -> f32 {
    let row = row_at(body, top);
    modal::label(ui.painter(), row, row.left(), name, color::TEXT_CONTROL, 12.5);
    let field = Rect::from_min_size(
        Pos2::new(row.left() + 110.0, row.top()),
        Vec2::new((row.width() - 110.0).max(120.0), 26.0),
    );
    modal::field(ui, field, name, value);
    top + 36.0
}

fn row_at(body: Rect, top: f32) -> Rect {
    Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), 26.0))
}
