//! The one text prompt, and the one confirmation, that the rest of Quill asks with.
//!
//! New File, Rename, New Branch, New Tag, Stash Changes and Clone all need a line of text and a
//! name for what they are about to do, and Rollback, a hard Reset and dropping a stash all need a
//! question with two answers. Six prompts and three confirmations would be nine dialogs that
//! almost agree; these are two, laid out the way `design/style-guide.md` says a modal is laid out.
//!
//! Neither knows what it is for. The window holds a [`Prompt`] with a [`Purpose`] on it, and
//! [`QuillApp::run_action`] decides what a confirmed prompt means, so a prompt cannot do anything by
//! itself.

use std::path::PathBuf;

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::components::modal;
use crate::theme::{color, size};

const WIDTH: f32 = 460.0;
const HEIGHT: f32 = 190.0;
const HEADER: f32 = 46.0;
const FOOTER: f32 = 52.0;

/// What a prompt is for, which is what the window looks at when it is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// Render the typed HTTP address or HTML path in a browser tab.
    OpenWebAddress,
    /// Make an empty file in this folder, with the name that is typed.
    NewFile(PathBuf),
    /// Make a folder inside this one, with the name that is typed.
    NewFolder(PathBuf),
    /// Rename this file or folder to the name that is typed.
    Rename(PathBuf),
    /// Start a branch with the name that is typed, from the current one.
    NewBranch,
    /// Tag the current commit with the name that is typed.
    NewTag,
    /// Put the changes away with the message that is typed.
    Stash,
    /// Clone the repository at the address that is typed.
    Clone,
    /// Show what changed against the revision that is typed.
    CompareWithRevision(PathBuf),
    /// Move the branch to the revision that is typed, in the mode already chosen.
    ResetTo(&'static str),
    /// Call this terminal tab, counting from 0, whatever is typed.
    ///
    /// The number rather than "the tab that is showing", because a prompt is answered on some later
    /// frame and the tab that is showing is a thing that can change in between.
    RenameTerminalTab(usize),
}

/// A prompt the window is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: String,
    /// A line under the title saying what will happen.
    pub note: String,
    /// What is in the field. Seeded with something sensible, and selected when the prompt opens.
    pub value: String,
    /// The word on the button that does it.
    pub confirm: String,
    pub purpose: Purpose,
}

impl Prompt {
    pub fn new(title: &str, note: &str, value: &str, confirm: &str, purpose: Purpose) -> Self {
        Self {
            title: title.to_owned(),
            note: note.to_owned(),
            value: value.to_owned(),
            confirm: confirm.to_owned(),
            purpose,
        }
    }
}

/// What happened in the prompt this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PromptOutcome {
    /// The button was pressed, or Enter typed, with this in the field.
    pub confirmed: bool,
    /// Cancel, Escape, or the close cross.
    pub cancelled: bool,
}

/// Draw the prompt. The caller owns whether there is one at all.
pub fn show(ctx: &egui::Context, prompt: &mut Prompt) -> PromptOutcome {
    let mut outcome = PromptOutcome::default();
    let closed = modal("quill-prompt", ctx, |ui, area| {
        header(ui, area, &prompt.title, &mut outcome.cancelled);
        let body = body_rect(area);
        note(ui, body, &prompt.note);

        let field = Rect::from_min_size(
            Pos2::new(body.left(), body.top() + 40.0),
            Vec2::new(body.width(), 30.0),
        );
        ui.painter().rect(
            field,
            CornerRadius::same(size::CONTROL_CORNER),
            color::field(),
            Stroke::new(1.0, color::control_border()),
            egui::StrokeKind::Inside,
        );
        let entry_id = ui.id().with("prompt-dialog-value");
        let text_rect =
            crate::components::controls::field_takes_the_whole_rectangle(ui, field, 8.0, entry_id);
        let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        let entry = edit.add(
            egui::TextEdit::singleline(&mut prompt.value)
                .id(entry_id)
                .frame(egui::Frame::NONE)
                .desired_width(text_rect.width())
                .text_color(color::text_control()),
        );
        entry.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Name")
        });
        // The field has the keyboard as soon as the prompt opens, because a prompt that has to be
        // clicked before it can be typed into is a prompt that gets typed past.
        if !entry.has_focus() {
            entry.request_focus();
        }
        if entry.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            outcome.confirmed = true;
        }

        buttons(ui, area, &prompt.confirm, !prompt.value.trim().is_empty(), &mut outcome);
    });
    if closed {
        outcome.cancelled = true;
    }
    outcome
}

/// A question with two answers, for anything that cannot be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    pub title: String,
    pub note: String,
    pub confirm: String,
    /// What to do when it is confirmed. The window holds the action rather than the dialog.
    pub purpose: String,
}

/// Draw a confirmation.
pub fn confirm(ctx: &egui::Context, question: &Confirmation) -> PromptOutcome {
    let mut outcome = PromptOutcome::default();
    let closed = modal("quill-confirm", ctx, |ui, area| {
        header(ui, area, &question.title, &mut outcome.cancelled);
        note(ui, body_rect(area), &question.note);
        buttons(ui, area, &question.confirm, true, &mut outcome);
    });
    if closed {
        outcome.cancelled = true;
    }
    outcome
}

/// The frame both of them are drawn in, which is the frame `design/style-guide.md` describes.
///
/// `components::modal` owns that frame, and owns the dragging and the resizing with it, so the
/// prompt and the confirmation are moved and resized exactly as the Settings window and the git
/// dialogs are. This function is what remains of the copy that used to live here: the two sizes, and
/// the name the placement is remembered under.
fn modal(id: &str, ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui, Rect)) -> bool {
    let (_, close) = modal::show(ctx, id, WIDTH, HEIGHT, |ui, area| contents(ui, area));
    close
}

fn body_rect(area: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(area.left() + 20.0, area.top() + HEADER + 16.0),
        Pos2::new(area.right() - 20.0, area.bottom() - FOOTER),
    )
}

fn header(ui: &mut egui::Ui, area: Rect, title: &str, cancelled: &mut bool) {
    let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER));
    let painter = ui.painter_at(area);
    painter.rect_filled(bar, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, color::title_bar());
    let galley =
        painter.layout_no_wrap(title.to_owned(), egui::FontId::proportional(13.0), color::text_strong());
    painter.galley(
        Pos2::new(area.left() + 20.0, bar.center().y - galley.size().y / 2.0),
        galley,
        color::text_strong(),
    );
    let close = Rect::from_center_size(Pos2::new(area.right() - 24.0, bar.center().y), Vec2::splat(22.0));
    if crate::components::controls::icon_button(ui, close, "Close", crate::theme::icon::cross) {
        *cancelled = true;
    }
    painter.line_segment(
        [Pos2::new(bar.left(), bar.bottom()), Pos2::new(bar.right(), bar.bottom())],
        Stroke::new(1.0, color::divider()),
    );
}

fn note(ui: &mut egui::Ui, body: Rect, text: &str) {
    let painter = ui.painter_at(body.expand(40.0));
    let galley = painter.layout(
        text.to_owned(),
        egui::FontId::proportional(11.5),
        color::text_faint(),
        body.width(),
    );
    painter.galley(body.left_top(), galley, color::text_faint());
}

fn buttons(
    ui: &mut egui::Ui,
    area: Rect,
    confirm: &str,
    enabled: bool,
    outcome: &mut PromptOutcome,
) {
    let footer = Rect::from_min_max(Pos2::new(area.left(), area.bottom() - FOOTER), area.max);
    ui.painter_at(area).line_segment(
        [Pos2::new(footer.left(), footer.top()), Pos2::new(footer.right(), footer.top())],
        Stroke::new(1.0, color::divider()),
    );
    let ok = Rect::from_min_size(
        Pos2::new(footer.right() - 20.0 - 104.0, footer.center().y - 14.0),
        Vec2::new(104.0, 28.0),
    );
    let cancel = Rect::from_min_size(
        Pos2::new(ok.left() - 8.0 - 96.0, footer.center().y - 14.0),
        Vec2::new(96.0, 28.0),
    );
    if button(ui, cancel, "Cancel", true, false) {
        outcome.cancelled = true;
    }
    // Enter presses the button that does the thing, which is what `components::modal::footer` does
    // for every other modal and is asked here through the same function so the two cannot drift.
    // It is what answers the **confirmation**, which has no field to lose focus and so had no way
    // of being answered from the keyboard at all.
    if button(ui, ok, confirm, enabled, true) || (enabled && modal::Confirm::Enter.pressed(ui)) {
        outcome.confirmed = true;
    }
}

/// A button with a word in it. The one that does the thing is filled in the accent colour, so the
/// two answers to a question do not look the same.
fn button(ui: &mut egui::Ui, area: Rect, name: &str, enabled: bool, primary: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(area, ui.id().with(("prompt-button", name)), sense);
    let fill = match (enabled, primary, response.hovered()) {
        (false, _, _) => color::control().gamma_multiply(0.6),
        (true, true, _) => color::accent(),
        (true, false, true) => color::control().gamma_multiply(1.25),
        (true, false, false) => color::control(),
    };
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        fill,
        Stroke::new(1.0, if primary { color::accent() } else { color::control_border() }),
        egui::StrokeKind::Inside,
    );
    let tint = if enabled { color::text_strong() } else { color::text_faint() };
    let galley = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), tint);
    painter.galley(area.center() - galley.size() / 2.0, galley, tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    response.clicked()
}
