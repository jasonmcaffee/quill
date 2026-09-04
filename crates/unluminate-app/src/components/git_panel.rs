//! The commit panel, laid out like the reference editor's Commit tool window.
//!
//! What the reference capture in `tasks/unluminate-ide-tdd.md` holds, and what this draws:
//!
//! - a `Commit` / `Stashes` tab strip. The reference editor calls the second one `Shelf`; Unluminate has stashes, so
//!   the tab is named for what it actually is rather than for what its makers call theirs.
//! - a changes tree: a repository row carrying the branch in a chip, then one row a file with a tick
//!   box, a marker, its name, and its folder dimmed after it.
//! - a second group, `Unversioned Files`, holding what git is not tracking yet.
//! - the `Amend` tick, the counts at the right, the message box, and the two buttons.
//!
//! **Ticking a file stages it, at once.** The alternative — remembering ticks in the window and
//! staging everything at the moment of commit — means Unluminate's idea of what is staged and git's
//! disagree for as long as the panel is open, and anyone who runs `git status` in Unluminate's own
//! terminal meanwhile sees the disagreement. Staging as you tick keeps one truth.

use egui::{Pos2, Rect, Sense, Vec2};
use unluminate_git::status::Entry;
use unluminate_git::Status;

use crate::components::modal;
use crate::theme::{color, icon, size};

const WIDTH: f32 = 780.0;
const HEIGHT: f32 = 620.0;
/// How tall the message box is.
const MESSAGE: f32 = 120.0;

/// Which of the panel's two tabs is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Commit,
    Stashes,
}

/// The panel's own state, which lives in the window.
#[derive(Debug, Default)]
pub struct CommitPanel {
    pub open: bool,
    pub tab: Tab,
    /// What is being typed as the commit message.
    pub message: String,
    /// Change the commit before this one instead of making a new one.
    pub amend: bool,
    /// The file whose diff is showing.
    pub selected: Option<String>,
    /// That file's diff, once it has come back from the worker.
    pub diff: String,
    /// True while the list of recent messages is showing.
    pub showing_messages: bool,
    /// True when the unversioned group is open.
    pub unversioned_open: bool,
}

impl CommitPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.tab = Tab::Commit;
    }
}

/// What the user did in the panel.
#[derive(Debug, Default, PartialEq)]
pub struct CommitOutcome {
    pub close: bool,
    /// Stage these paths.
    pub stage: Vec<String>,
    /// Unstage these paths.
    pub unstage: Vec<String>,
    /// Show this file's diff.
    pub show: Option<String>,
    /// Commit, and push as well when true.
    pub commit: Option<bool>,
    /// Throw away the changes to these paths.
    pub rollback: Vec<String>,
    /// Read the repository again.
    pub refresh: bool,
    /// Put a stash back, and take it off the list when true.
    pub unstash: Option<(String, bool)>,
    /// Drop a stash.
    pub drop_stash: Option<String>,
}

/// Draw the panel. Does nothing when it is not open.
pub fn show(
    ctx: &egui::Context,
    panel: &mut CommitPanel,
    status: &Status,
    stashes: &[unluminate_git::Stash],
    repository: &str,
    recent: &[String],
) -> CommitOutcome {
    let mut outcome = CommitOutcome::default();
    if !panel.open {
        return outcome;
    }
    let (_, closed) = modal::show(ctx, "unluminate-commit", WIDTH, HEIGHT, |ui, area| {
        if modal::header(ui, area, "Commit") {
            outcome.close = true;
        }
        let body = modal::body(area);
        let after_tabs = tabs(ui, body, panel);
        let rest = Rect::from_min_max(Pos2::new(body.left(), after_tabs), body.max);
        match panel.tab {
            Tab::Commit => commit_tab(ui, area, rest, panel, status, repository, recent, &mut outcome),
            Tab::Stashes => stashes_tab(ui, area, rest, stashes, &mut outcome),
        }
    });
    if closed {
        outcome.close = true;
    }
    if outcome.close {
        panel.open = false;
    }
    outcome
}

/// The `Commit` / `Stashes` strip, and the small toolbar under it.
fn tabs(ui: &mut egui::Ui, body: Rect, panel: &mut CommitPanel) -> f32 {
    let mut pen = body.left();
    for (tab, name) in [(Tab::Commit, "Commit"), (Tab::Stashes, "Stashes")] {
        let width = (name.chars().count() as f32 * 7.5 + 24.0).max(70.0);
        let rect = Rect::from_min_size(Pos2::new(pen, body.top()), Vec2::new(width, 26.0));
        let response = ui.interact(rect, ui.id().with(("commit-tab", name)), Sense::click());
        let chosen = panel.tab == tab;
        if chosen {
            ui.painter().rect(
                rect,
                egui::CornerRadius::same(size::CONTROL_CORNER),
                color::selected_row(),
                egui::Stroke::new(1.0, color::accent()),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, egui::CornerRadius::same(size::CONTROL_CORNER), color::control());
        }
        let tint = if chosen { color::text_strong() } else { color::text_control() };
        modal::label(ui.painter(), rect, rect.left() + 12.0, name, tint, 12.5);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, name)
        });
        if response.clicked() {
            panel.tab = tab;
        }
        pen += width + 6.0;
    }
    body.top() + 38.0
}

/// The changes tree, the message box and the two buttons.
fn commit_tab(
    ui: &mut egui::Ui,
    area: Rect,
    rest: Rect,
    panel: &mut CommitPanel,
    status: &Status,
    repository: &str,
    recent: &[String],
    outcome: &mut CommitOutcome,
) {
    let list_height = (rest.height() - MESSAGE - 74.0).max(120.0);
    let list = Rect::from_min_size(rest.min, Vec2::new(rest.width(), list_height));
    changes_tree(ui, list, panel, status, repository, outcome);

    // The amend tick and the counts, on one line under the list.
    let counts = Rect::from_min_size(
        Pos2::new(rest.left(), list.bottom() + 8.0),
        Vec2::new(rest.width(), 24.0),
    );
    ui.painter().line_segment(
        [Pos2::new(counts.left(), counts.top()), Pos2::new(counts.right(), counts.top())],
        egui::Stroke::new(1.0, color::divider()),
    );
    let amend_row = Rect::from_min_size(counts.min + Vec2::new(0.0, 4.0), Vec2::new(200.0, 20.0));
    modal::check(ui, amend_row, "Amend", &mut panel.amend);
    let summary = format!("{} added   {} modified", status.untracked_count(), status.modified_count());
    let galley = ui.painter().layout_no_wrap(
        summary,
        egui::FontId::proportional(11.5),
        color::git_added(),
    );
    ui.painter().galley(
        Pos2::new(counts.right() - galley.size().x, counts.center().y - galley.size().y / 2.0 + 4.0),
        galley,
        color::git_added(),
    );

    // The message box, with a clock button offering the last few messages.
    let message = Rect::from_min_size(
        Pos2::new(rest.left(), counts.bottom() + 8.0),
        Vec2::new(rest.width(), MESSAGE),
    );
    ui.painter().rect(
        message,
        egui::CornerRadius::same(size::CONTROL_CORNER),
        color::field(),
        egui::Stroke::new(1.0, color::accent().gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    let message_id = ui.id().with("git-commit-message");
    let text_rect = message.shrink(8.0);
    // The eight points of margin round the box are part of the control, so a press in them hands it
    // the keyboard rather than leaving the pane behind holding the keys — `task-1795`.
    crate::components::controls::claim_the_field(ui, message, message_id);
    let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = edit.add(
        egui::TextEdit::multiline(&mut panel.message)
            .id(message_id)
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .desired_rows(5)
            .text_color(color::text_control()),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Commit message")
    });
    recent_messages(ui, counts, panel, recent);

    let staged = status.staged_count() > 0;
    let has_message = !panel.message.trim().is_empty();
    let can_commit = staged && has_message;
    // Command and Enter rather than Enter: the message above is a multiline field, where Enter is a
    // new line and has to stay one. The reference editor's commit dialog says the same thing.
    match modal::footer_confirmed_by(
        ui,
        area,
        &[("COMMIT AND PUSH...", can_commit), ("COMMIT", can_commit)],
        modal::Confirm::CommandEnter,
    ) {
        Some(0) => outcome.commit = Some(true),
        Some(1) => outcome.commit = Some(false),
        _ => {}
    }
}

/// The clock button, and the list of recent commit messages it opens.
fn recent_messages(ui: &mut egui::Ui, counts: Rect, panel: &mut CommitPanel, recent: &[String]) {
    let clock = Rect::from_center_size(
        Pos2::new(counts.left() + 148.0, counts.center().y + 4.0),
        Vec2::splat(20.0),
    );
    let response = ui
        .interact(clock, ui.id().with("commit-history"), Sense::click())
        .on_hover_text("Recent commit messages");
    icon::clock(ui.painter(), clock.center(), color::text_dim());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Recent commit messages")
    });
    if response.clicked() {
        panel.showing_messages = !panel.showing_messages;
    }
    if !panel.showing_messages {
        return;
    }
    let chosen = egui::Popup::new(
        egui::Id::new("unluminate-commit-messages"),
        ui.ctx().clone(),
        clock.left_bottom(),
        ui.layer_id(),
    )
    .width(420.0)
    .show(|ui| {
        let mut chosen = None;
        for (index, message) in recent.iter().enumerate() {
            let first = message.lines().next().unwrap_or_default().to_owned();
            if ui.selectable_label(false, &first).clicked() {
                chosen = Some(index);
            }
        }
        if recent.is_empty() {
            ui.label(egui::RichText::new("No commits yet.").color(color::text_faint()));
        }
        chosen
    })
    .and_then(|inner| inner.inner);
    if let Some(index) = chosen {
        panel.message = recent[index].clone();
        panel.showing_messages = false;
    }
}

/// The tree: the repository row with its branch chip, the changed files, and the unversioned group.
fn changes_tree(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut CommitPanel,
    status: &Status,
    repository: &str,
    outcome: &mut CommitOutcome,
) {
    ui.painter().rect(
        area,
        egui::CornerRadius::same(size::CONTROL_CORNER),
        color::explorer_footer(),
        egui::Stroke::new(1.0, color::divider()),
        egui::StrokeKind::Inside,
    );
    let tracked: Vec<&Entry> = status.entries.iter().filter(|entry| !entry.untracked()).collect();
    let untracked: Vec<&Entry> = status.entries.iter().filter(|entry| entry.untracked()).collect();
    let inner = area.shrink(4.0);
    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    list.set_clip_rect(inner);
    egui::ScrollArea::vertical().id_salt("commit-changes").show(&mut list, |ui| {
        let all_staged = !tracked.is_empty() && tracked.iter().all(|entry| entry.staged());
        let heading = format!("Changes  {} files", tracked.len());
        let branch = status.branch.clone().unwrap_or_else(|| "detached".to_owned());
        if group_row(ui, "changes", &heading, all_staged, Some((repository, &branch))) {
            let paths: Vec<String> = tracked.iter().map(|entry| entry.path.clone()).collect();
            if all_staged {
                outcome.unstage = paths;
            } else {
                outcome.stage = paths;
            }
        }
        for entry in &tracked {
            file_row(ui, entry, panel, outcome, 1);
        }

        if !untracked.is_empty() {
            let all_staged = untracked.iter().all(|entry| entry.staged());
            let heading = format!("Unversioned Files  {} files", untracked.len());
            if group_row(ui, "unversioned", &heading, all_staged, None) {
                let paths: Vec<String> = untracked.iter().map(|entry| entry.path.clone()).collect();
                if all_staged {
                    outcome.unstage = paths;
                } else {
                    outcome.stage = paths;
                }
            }
            for entry in &untracked {
                file_row(ui, entry, panel, outcome, 1);
            }
        }
        if status.is_clean() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  Nothing has changed.").size(11.5).color(color::text_faint()));
        }
    });
}

/// A heading row with a tick box that ticks everything under it.
fn group_row(
    ui: &mut egui::Ui,
    id: &str,
    heading: &str,
    ticked: bool,
    repository: Option<(&str, &str)>,
) -> bool {
    let mut clicked = false;
    let response = modal::row(ui, id, heading, false, |painter, row| {
        tick(painter, Pos2::new(row.left() + 14.0, row.center().y), ticked);
        let mut x = modal::label(painter, row, row.left() + 30.0, heading, color::text_strong(), 12.0);
        if let Some((name, branch)) = repository {
            x = modal::label(painter, row, x + 14.0, name, color::text_control(), 12.0);
            // The branch chip, drawn the way the capture shows it.
            let galley =
                painter.layout_no_wrap(branch.to_owned(), egui::FontId::proportional(11.0), color::text_strong());
            let chip = Rect::from_min_size(
                Pos2::new(x + 12.0, row.center().y - 9.0),
                Vec2::new(galley.size().x + 14.0, 18.0),
            );
            painter.rect_filled(chip, egui::CornerRadius::same(4), color::control());
            painter.galley(
                Pos2::new(chip.left() + 7.0, chip.center().y - galley.size().y / 2.0),
                galley,
                color::text_strong(),
            );
        }
    });
    if response.clicked() {
        clicked = true;
    }
    clicked
}

/// One file: its tick box, a marker in its state's colour, its name, and its folder dimmed after it.
fn file_row(
    ui: &mut egui::Ui,
    entry: &Entry,
    panel: &mut CommitPanel,
    outcome: &mut CommitOutcome,
    depth: usize,
) {
    let staged = entry.staged();
    let chosen = panel.selected.as_deref() == Some(entry.path.as_str());
    let marker = if entry.conflicted() {
        color::close()
    } else if entry.untracked() {
        color::git_untracked()
    } else {
        color::git_modified()
    };
    let name = entry.name().to_owned();
    let folder = entry.folder().to_owned();
    let response = modal::row(ui, &entry.path, &entry.path, chosen, |painter, row| {
        let left = row.left() + 14.0 + depth as f32 * size::INDENT;
        tick(painter, Pos2::new(left, row.center().y), staged);
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(left + 18.0, row.center().y), Vec2::splat(8.0)),
            egui::CornerRadius::same(2),
            marker,
        );
        let x = modal::label(painter, row, left + 28.0, &name, color::text_control(), 12.0);
        if !folder.is_empty() {
            modal::label(painter, row, x + 12.0, &folder, color::text_faint(), 11.0);
        }
    });
    // The tick box takes a click of its own; anywhere else on the row shows the file's diff.
    let box_rect = Rect::from_center_size(
        Pos2::new(response.rect.left() + 14.0 + depth as f32 * size::INDENT, response.rect.center().y),
        Vec2::splat(18.0),
    );
    let pointer = response.interact_pointer_pos();
    if response.clicked() {
        if pointer.is_some_and(|at| box_rect.contains(at)) {
            if staged {
                outcome.unstage.push(entry.path.clone());
            } else {
                outcome.stage.push(entry.path.clone());
            }
        } else {
            panel.selected = Some(entry.path.clone());
            outcome.show = Some(entry.path.clone());
        }
    }
}

/// A tick box, drawn small enough to sit in a list row.
fn tick(painter: &egui::Painter, centre: Pos2, on: bool) {
    let rect = Rect::from_center_size(centre, Vec2::splat(15.0));
    painter.rect(
        rect,
        egui::CornerRadius::same(3),
        if on { color::accent() } else { color::field() },
        egui::Stroke::new(1.0, if on { color::accent() } else { color::control_border() }),
        egui::StrokeKind::Inside,
    );
    if on {
        icon::tick(painter, centre, color::text_strong());
    }
}

/// The `Stashes` tab: the list, and what can be done with the one that is chosen.
fn stashes_tab(
    ui: &mut egui::Ui,
    area: Rect,
    rest: Rect,
    stashes: &[unluminate_git::Stash],
    outcome: &mut CommitOutcome,
) {
    let list = Rect::from_min_size(rest.min, Vec2::new(rest.width(), rest.height() - 8.0));
    ui.painter().rect(
        list,
        egui::CornerRadius::same(size::CONTROL_CORNER),
        color::explorer_footer(),
        egui::Stroke::new(1.0, color::divider()),
        egui::StrokeKind::Inside,
    );
    let inner = list.shrink(4.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    let mut chosen: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("stash-list").show(&mut child, |ui| {
        for stash in stashes {
            let name = stash.name.clone();
            let message = stash.message.clone();
            let response = modal::row(ui, &stash.name, &stash.name, false, |painter, row| {
                let x = modal::label(painter, row, row.left() + 14.0, &name, color::text_strong(), 12.0);
                modal::label(painter, row, x + 14.0, &message, color::text_control(), 11.5);
            });
            if response.clicked() {
                chosen = Some(stash.name.clone());
            }
        }
        if stashes.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  Nothing is stashed.").size(11.5).color(color::text_faint()));
        }
    });
    // Acting on the newest stash is what `Unstash Changes` means when nothing is chosen, and
    // clicking a row acts on that one.
    let target = chosen.or_else(|| stashes.first().map(|stash| stash.name.clone()));
    let any = target.is_some();
    // Command and Enter, as the other tab of this modal uses: one modal, one key. Enter alone here
    // would pop a stash — the accent button is `POP` — for somebody who pressed it meaning nothing,
    // and the tab beside this one cannot take Enter at all because its message is a multiline field.
    match modal::footer_confirmed_by(
        ui,
        area,
        &[("DROP", any), ("APPLY", any), ("POP", any)],
        modal::Confirm::CommandEnter,
    ) {
        Some(0) => outcome.drop_stash = target,
        Some(1) => outcome.unstash = target.map(|name| (name, false)),
        Some(2) => outcome.unstash = target.map(|name| (name, true)),
        _ => {}
    }
}
