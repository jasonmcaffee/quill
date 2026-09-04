//! The two small modals debugging needs: editing a breakpoint, and evaluating an expression.
//!
//! Both are built from `components::modal` — the frame, the header, the body, the footer, the field
//! and the tick box — so they are dragged, resized, put back with a double click on the header and
//! answered with `Enter` without either of them asking. A tenth modal that drew its own header would
//! be a tenth modal that almost agreed with the other nine; these are the eleventh and twelfth, and
//! they agree.
//!
//! Neither knows what it is for. The window holds them and `UnluminateApp::run_action` decides what an
//! answered one means, which is the arrangement `prompt_dialog` already has.

use std::path::PathBuf;

use egui::{Pos2, Rect, Vec2};

use crate::components::modal;
use crate::theme::color;

/// How large `Edit Breakpoint` is. Two fields and a tick box, and the fields are one line each.
const BREAKPOINT_WIDTH: f32 = 480.0;
const BREAKPOINT_HEIGHT: f32 = 300.0;
/// How large `Evaluate Expression` is. Taller, because the answer is what is being read.
const EVALUATE_WIDTH: f32 = 540.0;
const EVALUATE_HEIGHT: f32 = 320.0;
/// How tall one row of a modal's body is.
const ROW: f32 = 30.0;

/// The breakpoint being edited: which one, and what has been typed about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointDialog {
    /// The file it is in, and the byte offset of its line — which is how the window finds it again
    /// when the modal is answered, since a person can type into another tab in between.
    pub path: PathBuf,
    pub offset: usize,
    /// The one-based line, for the title. Read when the modal opens rather than when it is answered,
    /// because a line number is only true of the text as it is now.
    pub line: usize,
    pub enabled: bool,
    pub condition: String,
    pub log_message: String,
    /// Whether the adapter said it can do conditions and logpoints. A field whose capability is
    /// absent is **absent**, not dimmed — the rule the `F` button already follows — and with no
    /// session running both are offered, because a breakpoint edited now is one a debugger will be
    /// asked about later.
    pub conditions: bool,
    pub log_points: bool,
    /// True when opening the modal is what put this breakpoint there, so cancelling takes it away
    /// again. Somebody who right clicked an empty line, chose `Add Conditional Breakpoint...` and
    /// then thought better of it has not asked for a plain breakpoint.
    pub created: bool,
}

impl BreakpointDialog {
    /// What the window does with it once the modal is answered.
    pub fn condition(&self) -> Option<String> {
        text_of(&self.condition)
    }

    pub fn log(&self) -> Option<String> {
        text_of(&self.log_message)
    }
}

/// What happened in a modal this frame. The same shape both of them report.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DialogOutcome {
    pub confirmed: bool,
    pub cancelled: bool,
    /// The breakpoint modal's third button, which takes it away.
    pub removed: bool,
}

/// Draw `Edit Breakpoint`.
pub fn breakpoint(ctx: &egui::Context, dialog: &mut BreakpointDialog) -> DialogOutcome {
    let mut outcome = DialogOutcome::default();
    let (_, closed) = modal::show(ctx, "unluminate-breakpoint", BREAKPOINT_WIDTH, BREAKPOINT_HEIGHT, |ui, area| {
        if modal::header(ui, area, &format!("Breakpoint \u{2014} line {}", dialog.line)) {
            outcome.cancelled = true;
        }
        let body = modal::body(area);
        let mut top = body.top();
        top = modal::note(
            ui,
            body,
            top,
            &format!("In {}.", file_name(&dialog.path)),
        );
        top += 6.0;

        let tick = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), ROW));
        modal::check(ui, tick, "Enabled", &mut dialog.enabled);
        top += ROW + 10.0;

        // Offered only when the adapter said it can, which is the rule every optional control here
        // follows. The condition and the log message are **data in the request**: the adapter does
        // the evaluating and the logging, so Unluminate's whole cost for two of the reference editor's features is
        // these two fields.
        if dialog.conditions {
            top = modal::section(ui, body, top, "Condition");
            let field = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), ROW));
            modal::field(ui, field, "Condition", &mut dialog.condition);
            top += ROW + 4.0;
            top = modal::note(
                ui,
                body,
                top,
                "An expression in the program's own language. It stops only while this is true.",
            );
            top += 6.0;
        }
        if dialog.log_points {
            top = modal::section(ui, body, top, "Log message");
            let field = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), ROW));
            modal::field(ui, field, "Log message", &mut dialog.log_message);
            top += ROW + 4.0;
            modal::note(
                ui,
                body,
                top,
                "Printed instead of stopping. The debugger formats it, so {name} reads a variable.",
            );
        }

        // Enter answers it, which `modal::footer` makes true of every dialog built from the
        // furniture. The fields own no key of their own, so the plain `Enter` is right here.
        match modal::footer(ui, area, &[("Remove", true), ("Cancel", true), ("Save", true)]) {
            Some(0) => outcome.removed = true,
            Some(1) => outcome.cancelled = true,
            Some(2) => outcome.confirmed = true,
            _ => {}
        }
    });
    if closed {
        outcome.cancelled = true;
    }
    outcome
}

/// The expression box: what was typed, and what the debugger said about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluateDialog {
    /// What is in the field. Seeded with the editor's selection when there was one, which is
    /// The reference editor's own behaviour.
    pub expression: String,
    /// The last answer, once one has arrived: the value, or the debugger's own refusal.
    pub result: Option<Result<String, String>>,
    /// Set while the question has been asked and not answered.
    pub asking: bool,
}

/// Draw `Evaluate Expression`.
pub fn evaluate(ctx: &egui::Context, dialog: &mut EvaluateDialog, paused: bool) -> DialogOutcome {
    let mut outcome = DialogOutcome::default();
    let (_, closed) = modal::show(ctx, "unluminate-evaluate", EVALUATE_WIDTH, EVALUATE_HEIGHT, |ui, area| {
        if modal::header(ui, area, "Evaluate Expression") {
            outcome.cancelled = true;
        }
        let body = modal::body(area);
        let mut top = body.top();
        let field = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), ROW));
        let entry = modal::field(ui, field, "Expression", &mut dialog.expression);
        // The field has the keyboard as soon as the modal opens, because one that has to be clicked
        // before it can be typed into is one that gets typed past — `prompt_dialog`'s rule.
        if !entry.has_focus() {
            entry.request_focus();
        }
        top += ROW + 12.0;
        top = modal::section(ui, body, top, "Result");
        let answer = Rect::from_min_max(Pos2::new(body.left(), top), body.max);
        show_result(ui, answer, dialog, paused);
        match modal::footer(ui, area, &[("Close", true), ("Evaluate", paused && !dialog.expression.trim().is_empty())]) {
            Some(0) => outcome.cancelled = true,
            Some(1) => outcome.confirmed = true,
            _ => {}
        }
    });
    if closed {
        outcome.cancelled = true;
    }
    outcome
}

/// The answer area: the value, the debugger's own refusal, or the reason there is nothing yet.
fn show_result(ui: &egui::Ui, area: Rect, dialog: &EvaluateDialog, paused: bool) {
    let (text, tint) = match (&dialog.result, dialog.asking, paused) {
        (_, true, _) => ("\u{2026}".to_owned(), color::text_faint()),
        (Some(Ok(value)), _, _) => (value.clone(), color::text()),
        // The debugger's own message, shown as it was written: it explains a bad expression far
        // better than Unluminate could, which is the rule `unluminate-git` keeps about git's standard error.
        (Some(Err(problem)), _, _) => (problem.clone(), color::close()),
        (None, _, false) => (
            "The program has to be stopped for an expression to be evaluated in it.".to_owned(),
            color::text_faint(),
        ),
        (None, _, true) => ("Type an expression and press Enter.".to_owned(), color::text_faint()),
    };
    let painter = ui.painter_at(area);
    let galley = painter.layout(text, egui::FontId::monospace(12.0), tint, area.width());
    painter.galley(area.min, galley, tint);
}

/// A value that is there and has something in it. Blank is the same as absent, which is the rule the
/// wire keeps about a `SourceBreakpoint`.
fn text_of(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The last part of a path, which is what the note names.
fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> BreakpointDialog {
        BreakpointDialog {
            path: PathBuf::from("src/main.rs"),
            offset: 1204,
            line: 14,
            enabled: true,
            condition: String::new(),
            log_message: String::new(),
            conditions: true,
            log_points: true,
            created: false,
        }
    }

    #[test]
    fn a_blank_field_is_the_same_as_no_field_at_all() {
        let mut edited = dialog();
        assert_eq!(edited.condition(), None);
        edited.condition = "   ".to_owned();
        assert_eq!(edited.condition(), None, "a person who thought better of it asked for nothing");
        edited.condition = "  attempts > 3 ".to_owned();
        assert_eq!(edited.condition(), Some("attempts > 3".to_owned()), "and it is trimmed");
    }

    #[test]
    fn the_modal_remembers_which_breakpoint_it_is_about_by_where_it_is() {
        let edited = dialog();
        assert_eq!(edited.offset, 1204);
        assert_eq!(file_name(&edited.path), "main.rs");
    }

    #[test]
    fn an_expression_box_starts_with_nothing_asked_and_nothing_answered() {
        let box_ = EvaluateDialog::default();
        assert!(box_.expression.is_empty());
        assert!(box_.result.is_none());
        assert!(!box_.asking);
    }
}
