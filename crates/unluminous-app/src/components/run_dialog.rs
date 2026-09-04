//! The `Run Configurations` modal: the list on the left, the four fields on the right.
//!
//! `tasks/task-1683-run-configurations-tdd.md` §7. Built from `components::modal` — the frame, the
//! header, the body rectangle, the footer and the buttons every other modal is built from — so
//! dragging it, resizing it and answering it with Enter all come for free. A tenth modal that drew
//! its own header would be a tenth modal that almost agreed with the other nine.
//!
//! Two columns, the Settings window's own shape: the configurations down the left with `Add` and
//! `Remove` under them, and the chosen one's four fields on the right. A **temporary** shows the
//! same fields plus a `Save` button that promotes it, because that is the one thing a temporary can
//! be that a permanent cannot.
//!
//! Every field has a plain name — `Run configuration name`, `Run configuration command` — because
//! the screenshot tests find controls by name rather than by position, and a control with no name
//! cannot be tested at all.
//!
//! **Nothing here changes what is on disk.** The fields are edited in place on the model the window
//! holds, and `UnluminousApp` writes `.unluminous/run-configurations.conf` when the dialog closes — by the
//! released binary only, which is the rule the project state already keeps.

use egui::{Pos2, Rect, Vec2};

use crate::components::modal;
use crate::services::run_configurations::{Configuration, Origin, RunConfigurations};
use crate::theme::color;

/// How large the modal is before it is shrunk to fit a small Unluminous window.
///
/// Wide enough for a command line to be read without scrolling — which is the field people will
/// actually look at — and tall enough for a list of a dozen configurations beside four fields.
const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 420.0;
/// How wide the list of configurations is.
const LIST_WIDTH: f32 = 220.0;
/// How tall the strip of `Add` and `Remove` under the list is.
const LIST_FOOTER: f32 = 34.0;

/// What the dialog is showing: whether it is open, and which configuration is chosen.
///
/// The chosen one is held **by name** rather than by index, because adding, removing and promoting
/// all move the indices under it and a stale index is a dialog editing the wrong row. A name that
/// nothing holds any more simply chooses nothing, which is what the list already draws.
#[derive(Debug, Clone, Default)]
pub struct RunDialog {
    pub open: bool,
    pub chosen: Option<String>,
}

impl RunDialog {
    /// Open it, choosing `name` — the widget's own selection, so the dialog opens on what the
    /// person was looking at.
    pub fn open(&mut self, name: Option<String>) {
        self.open = true;
        if name.is_some() {
            self.chosen = name;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

/// What the dialog asked the window to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DialogOutcome {
    /// The modal was closed, so what was edited is written down.
    pub closed: bool,
    /// A configuration was edited, added, removed or promoted, so the file is written.
    pub changed: bool,
    /// `Remove` was pressed on a configuration whose run is still going: the window asks the
    /// question first, because silently killing a server somebody is watching is worse than one
    /// extra click.
    pub confirm_removal: Option<String>,
    /// `Remove` was pressed on one that is not running, which needs no question.
    pub remove: Option<String>,
}

/// Draw the dialog, editing `configurations` in place.
///
/// `running` says which configurations have a run going, which is the one thing the dialog cannot
/// work out for itself and is what decides whether removing one asks a question first.
pub fn show(
    ctx: &egui::Context,
    dialog: &mut RunDialog,
    configurations: &mut RunConfigurations,
    running: &[String],
) -> DialogOutcome {
    let mut outcome = DialogOutcome::default();
    if !dialog.open {
        return outcome;
    }
    let (inner, should_close) = modal::show(ctx, "unluminous-run-configurations", WIDTH, HEIGHT, |ui, area| {
        contents(ui, area, dialog, configurations, running)
    });
    outcome = inner;
    if outcome.closed || should_close {
        dialog.close();
        outcome.closed = true;
    }
    outcome
}

fn contents(
    ui: &mut egui::Ui,
    area: Rect,
    dialog: &mut RunDialog,
    configurations: &mut RunConfigurations,
    running: &[String],
) -> DialogOutcome {
    let mut outcome = DialogOutcome::default();
    if modal::header(ui, area, "Run Configurations") {
        outcome.closed = true;
    }
    let body = modal::body(area);

    // A name that nothing holds any more chooses nothing, and with nothing chosen the first
    // configuration is, so opening the dialog always lands on something to edit.
    if dialog.chosen.as_deref().and_then(|name| configurations.find(name)).is_none() {
        dialog.chosen =
            configurations.listed().first().map(|(_, configuration)| configuration.name.clone());
    }

    let list = Rect::from_min_max(
        body.min,
        Pos2::new(body.left() + LIST_WIDTH, body.bottom() - LIST_FOOTER),
    );
    show_list(ui, list, dialog, configurations, running);
    let strip = Rect::from_min_max(
        Pos2::new(list.left(), list.bottom()),
        Pos2::new(list.right(), body.bottom()),
    );
    show_list_buttons(ui, strip, dialog, configurations, running, &mut outcome);

    let fields = Rect::from_min_max(Pos2::new(list.right() + 20.0, body.top()), body.max);
    show_fields(ui, fields, dialog, configurations, &mut outcome);

    // One button, and it says `Done`: every change takes effect as it is made, which is the choice
    // the Settings window already made and for the same reason — a dialog with `Apply` has to hold
    // a second copy of everything and decide what to do when the two disagree.
    if modal::footer(ui, area, &[("Done", true)]).is_some() {
        outcome.closed = true;
    }
    outcome
}

/// The list down the left.
fn show_list(
    ui: &mut egui::Ui,
    area: Rect,
    dialog: &mut RunDialog,
    configurations: &RunConfigurations,
    running: &[String],
) {
    ui.painter().rect_filled(area, egui::CornerRadius::same(6), color::explorer_footer());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink(4.0)));
    child.set_clip_rect(area);
    let mut chosen: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("run-configuration-list").show(&mut child, |ui| {
        for (origin, configuration) in configurations.listed() {
            let name = configuration.name.clone();
            let is_chosen = dialog.chosen.as_deref() == Some(name.as_str());
            let is_running = running.contains(&name);
            // A temporary is drawn in the quiet colour, exactly as it is in the widget's flyout:
            // it is something that was run rather than something somebody kept.
            let tint = match origin {
                Origin::Permanent => color::text_control(),
                Origin::Temporary | Origin::Suggested => color::text_dim(),
            };
            let label = name.clone();
            let response = modal::row(ui, &name, &name, is_chosen, move |painter, row| {
                let mut left = row.left() + 12.0;
                if is_running {
                    crate::theme::icon::state_dot(
                        painter,
                        Pos2::new(left + 4.0, row.center().y),
                        color::git_added(),
                    );
                }
                left += 14.0;
                modal::label(painter, row, left, &label, tint, 12.5);
            });
            if response.clicked() {
                chosen = Some(name);
            }
        }
        if configurations.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("  Nothing yet. Press Add.").size(11.5).color(color::text_faint()),
            );
        }
    });
    if let Some(name) = chosen {
        dialog.chosen = Some(name);
    }
}

/// `Add` and `Remove`, under the list.
fn show_list_buttons(
    ui: &mut egui::Ui,
    area: Rect,
    dialog: &mut RunDialog,
    configurations: &mut RunConfigurations,
    running: &[String],
    outcome: &mut DialogOutcome,
) {
    let width = (area.width() - 8.0) / 2.0;
    let add = Rect::from_min_size(Pos2::new(area.left(), area.top() + 4.0), Vec2::new(width, 26.0));
    if modal::button(ui, add, "Add", true, false) {
        let name = configurations.unused_name("Unnamed");
        configurations.add_permanent(Configuration::new(&name, "cargo run"));
        dialog.chosen = Some(name);
        outcome.changed = true;
    }
    let remove =
        Rect::from_min_size(Pos2::new(add.right() + 8.0, area.top() + 4.0), Vec2::new(width, 26.0));
    let chosen = dialog.chosen.clone();
    if modal::button(ui, remove, "Remove", chosen.is_some(), false) {
        if let Some(name) = chosen {
            // Removing one whose run is still going asks the question first, with the same
            // confirmation the git dialogs use: silently killing a server somebody is watching is
            // worse than one extra click.
            if running.contains(&name) {
                outcome.confirm_removal = Some(name);
            } else {
                outcome.remove = Some(name);
            }
        }
    }
}

/// The four fields on the right, and the `Save` a temporary has.
fn show_fields(
    ui: &mut egui::Ui,
    area: Rect,
    dialog: &RunDialog,
    configurations: &mut RunConfigurations,
    outcome: &mut DialogOutcome,
) {
    let Some(name) = dialog.chosen.clone() else {
        modal::note(ui, area, area.top() + 4.0, "Choose a configuration, or press Add to make one.");
        return;
    };
    let origin = configurations.find(&name).map(|(origin, _)| origin);
    let Some(configuration) = configurations.find_mut(&name) else {
        return;
    };
    let mut pen = area.top() + 4.0;
    for (label, plain, value) in [
        ("Name", "Run configuration name", &mut configuration.name),
        ("Command", "Run configuration command", &mut configuration.command),
        ("Directory", "Run configuration directory", &mut configuration.directory),
        ("Environment", "Run configuration environment", &mut configuration.env),
    ] {
        let row = Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), 26.0));
        modal::label(ui.painter(), row, row.left(), label, color::text_control(), 12.5);
        let field = Rect::from_min_size(
            Pos2::new(row.left() + 104.0, row.top()),
            Vec2::new((row.width() - 104.0).max(120.0), 26.0),
        );
        if modal::field(ui, field, plain, value).changed() {
            outcome.changed = true;
        }
        pen += 36.0;
    }

    // What the four fields mean, said once here rather than left to be discovered. The third
    // sentence is the one that matters: no shell runs the command line.
    pen = modal::note(
        ui,
        area,
        pen + 4.0,
        "The command is one line: the first word is the program and the rest are its arguments. \
         The directory is relative to the project, and empty means the project itself. \
         Environment variables are NAME=value pairs separated by semicolons.",
    );
    pen = modal::note(
        ui,
        area,
        pen,
        "No shell runs the command, so nothing is expanded and && is an argument. \
         Write pwsh -Command ... to ask for one.",
    );

    // A temporary is a configuration somebody ran without keeping. `Save` is what keeps it.
    if origin == Some(Origin::Temporary) {
        let save = Rect::from_min_size(Pos2::new(area.left(), pen + 4.0), Vec2::new(120.0, 26.0));
        if modal::button(ui, save, "Save", true, false) {
            configurations.promote(&name);
            outcome.changed = true;
        }
        modal::note(
            ui,
            Rect::from_min_max(Pos2::new(save.right() + 12.0, area.top()), area.max),
            save.top() + 5.0,
            "This one was made by running something. Save keeps it in the project.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_it_chooses_what_the_widget_had_chosen() {
        let mut dialog = RunDialog::default();
        dialog.open(Some("Dev server".to_owned()));
        assert!(dialog.open);
        assert_eq!(dialog.chosen.as_deref(), Some("Dev server"));
        // Opening it again with nothing chosen leaves the choice where it was, so a second visit
        // lands where the first one left off.
        dialog.close();
        dialog.open(None);
        assert_eq!(dialog.chosen.as_deref(), Some("Dev server"));
    }

    #[test]
    fn the_chosen_one_is_held_by_name_so_adding_and_removing_cannot_move_it() {
        // An index would be stale the moment something above it was removed, and a stale index is
        // a dialog editing the wrong row.
        let mut configurations = RunConfigurations::new();
        configurations.add_permanent(Configuration::new("one", "cargo run"));
        configurations.add_permanent(Configuration::new("two", "cargo test"));
        let mut dialog = RunDialog::default();
        dialog.open(Some("two".to_owned()));
        configurations.remove("one");
        assert_eq!(
            configurations.find(dialog.chosen.as_deref().expect("a choice")).map(|(_, c)| c.name.clone()),
            Some("two".to_owned())
        );
    }
}
