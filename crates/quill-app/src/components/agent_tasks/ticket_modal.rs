//! One ticket, in full: the modal the board opens a card into.
//!
//! `tasks/agent-tasks-ui-tdd.md` §2.4 is the list this is measured against, and §5 is the design. Two columns
//! inside one frame, which is what the browser board does, and the frame is `components::modal`'s — the same
//! header, body, footer, rows, fields and buttons the Settings window and the nine git dialogs are made of,
//! with the dragging and resizing `modal::show` already owns.
//!
//! ## The description is the editor
//!
//! It is a `quill_core::Document` and `components::editor_view` draws it, so the description gets Quill's own
//! editor: the same font, the same syntax colouring inside a code fence, the same undo, the same caret. That
//! is the largest saving in the plugin and it is the reason a task board inside a text editor is worth
//! building at all.
//!
//! ## Every field writes through one function
//!
//! Seven controls down the right are seven calls to `AgentTasks::edit_field`, so there is one place a column
//! is written and no second path to drift from it. `Model` and `Effort` are **absent** for a ticket assigned
//! to a person rather than disabled, which is Quill's rule and the one place this deliberately differs from
//! the browser.

use egui::{CornerRadius, Pos2, Rect, Vec2};

use super::text;
use crate::components::modal;
use crate::services::agent_tasks::model::{Assignee, Priority, Status, Task};
use crate::services::agent_tasks::{clock, AgentTasks, Field, EFFORTS};
use crate::services::plugin_ui::{Look, Request};

/// How large the modal asks to be. Wide enough for two columns and the terminal under the description.
/// How much of the window's width and height the modal leaves clear down each side.
///
/// **Five per cent, so the ticket is nearly the whole window.** It asked for a fixed 1080 by 720 before,
/// which on a large display was a small panel in the middle of a lot of dimmed background — and a ticket
/// is where the description is written, the todos are read and the agent's terminal is watched, which is
/// the most crowded thing on this board. Every other modal in Quill keeps its fixed size: they are a
/// question and two buttons, and a confirmation stretched across a display would be worse, not better.
const MARGIN_SHARE: f32 = 0.05;

/// The smallest it will ask for, whatever the window is.
///
/// A window dragged down to a few hundred points would otherwise ask for a modal too small to hold the
/// two columns, and `modal::fit` already clamps anything larger than the window, so a floor above the
/// window's own size costs nothing and reads correctly at every size in between.
const SMALLEST_WIDTH: f32 = 720.0;
const SMALLEST_HEIGHT: f32 = 520.0;

/// How big the modal asks to be in this window.
///
/// The window rather than the font size: the amount of room a ticket needs is the amount of room there
/// is. `look.scale()` still decides how tall the things *inside* it are, which is what a window set to
/// 48 point text needs.
pub fn size(ctx: &egui::Context, _look: &Look<'_>) -> (f32, f32) {
    let window = ctx.content_rect().size();
    (
        (window.x * (1.0 - MARGIN_SHARE * 2.0)).max(SMALLEST_WIDTH.min(window.x)),
        (window.y * (1.0 - MARGIN_SHARE * 2.0)).max(SMALLEST_HEIGHT.min(window.y)),
    )
}

/// How wide the column of fields down the right is, and the width below which it is dropped.
///
/// `components::modal` lets any dialog be resized down to 320 points, and 320 minus a fixed 260 point column
/// left six points for the description: the two columns became one unreadable one. Below [`TWO_COLUMNS`] the
/// fields go **under** the description instead, which is what the browser board's own narrow layout does.
const ASIDE_AT_DEFAULT: f32 = 300.0;
const TWO_COLUMNS_AT_DEFAULT: f32 = 720.0;
const PAD: f32 = 14.0;
/// How tall one field in the right column is.
const FIELD: f32 = 46.0;
/// How tall the comments section is: its count, two comments, the box and its two buttons.
const COMMENTS_AT_DEFAULT: f32 = 176.0;
/// How much of the room left under the description the agent's terminal takes, and its two bounds.
const TERMINAL_SHARE: f32 = 0.34;
const TERMINAL_SMALLEST: f32 = 200.0;
const TERMINAL_LARGEST: f32 = 560.0;
/// How much of a one column modal the fields take, when the dialog is too narrow for two columns.
const FIELDS_ALONE_AT_DEFAULT: f32 = 330.0;

/// What the modal reported.
#[derive(Debug, Default)]
pub struct Outcome {
    pub requests: Vec<Request>,
    /// The modal was closed, by its cross, by `Escape`, or by a click outside it.
    pub closed: bool,
}

/// Draw the ticket that is open in the detail, as a modal. Does nothing when none is.
pub fn show(board: &mut AgentTasks, ctx: &egui::Context, look: &Look<'_>) -> Outcome {
    let mut outcome = Outcome::default();
    let Some(task) = board.detail().task.clone() else {
        return outcome;
    };
    // A ticket nobody has named yet is a new one, and the footer says so: `Discard` deletes the row rather than
    // closing the modal, because `+ Add Task` created it before anybody typed.
    let new = board.detail().is_new;
    let (width, height) = size(ctx, look);
    let (inner, should_close) = modal::show(ctx, "agent-tasks-ticket", width, height, |ui, area| {
        contents(board, ui, area, look, &task, new)
    });
    outcome.requests = inner.requests;
    outcome.closed = inner.closed || should_close;
    outcome
}

fn contents(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
    task: &Task,
    new: bool,
) -> Outcome {
    let mut outcome = Outcome::default();
    // **The slot the decoration goes in, reserved before anything is drawn over it.** A modal is on a layer
    // of its own, so the window cannot reserve one from outside — see `plugin_ui::ChromeSlot`. Reserved even
    // when nothing is recording, because it costs one `Noop` and the alternative is a branch here and a
    // second one in the window.
    board.reserve_the_modals_canvas(ui, area);
    // **The key and the title**, which is what the page this is modelled on puts in its header: the key in a
    // dim monospaced face and the title beside it. The title used to be a field at the top of the left column
    // and the header held the key alone, so a ticket read as an untitled panel with a name buried in it.
    let (key, heading) = match new {
        true => (None, "New task".to_owned()),
        false => (Some(task.key.as_str()), board.detail().title_draft.clone()),
    };
    if modal::header_of(ui, area, key, &heading) {
        outcome.closed = true;
    }
    let body = modal::body(area);
    let footer = Rect::from_min_max(Pos2::new(area.min.x, body.max.y), area.max);

    // Two columns when there is room for two, and one when there is not: a fixed 260 point column beside a
    // dialog resized to its 320 point minimum left six points for everything else.
    // Both scaled by the font, because a column 260 points wide holds seven fields at 16 point text and two at
    // 48, and a modal that split into two columns at 720 points put a 48 point ticket's fields into a column
    // narrower than one of its own labels.
    if body.width() >= TWO_COLUMNS_AT_DEFAULT * look.scale() {
        let split = (body.max.x - ASIDE_AT_DEFAULT * look.scale()).round();
        let main = Rect::from_min_max(body.min, Pos2::new(split - PAD, body.max.y));
        let aside = Rect::from_min_max(Pos2::new(split, body.min.y), body.max);
        ui.painter().rect_filled(
            Rect::from_min_max(
                Pos2::new(split - PAD / 2.0, body.min.y),
                Pos2::new(split - PAD / 2.0 + 1.0, body.max.y),
            ),
            0,
            look.palette.divider,
        );
        outcome.requests.extend(left_column(board, ui, main, look, task, new));
        outcome.requests.extend(right_column(board, ui, aside, look, task, new));
    } else {
        // One column: the fields first, because at this width they are what a person came for — the description
        // is easier to read in a tab — and then whatever height is left goes to the rest.
        // **Never more than half of it**, and it scrolls inside whatever it gets — see `right_column`. A
        // fixed 330 points was a rectangle the fields plainly did not fit in, and nothing was clipped to it,
        // so the description below started underneath them.
        let wanted = (FIELDS_ALONE_AT_DEFAULT * look.scale()).min(body.height() * 0.5);
        let fields = Rect::from_min_max(body.min, Pos2::new(body.max.x, body.min.y + wanted));
        let rest = Rect::from_min_max(Pos2::new(body.min.x, fields.max.y + PAD), body.max);
        outcome.requests.extend(right_column(board, ui, fields, look, task, new));
        if rest.height() > 120.0 {
            outcome.requests.extend(left_column(board, ui, rest, look, task, new));
        }
    }

    // The footer. A new ticket gets `Discard` and `Done`; one that exists gets `Start work` and `Close`, and
    // `Delete` is in the right column with the rest of what happens to a ticket.
    // The second of each pair is **enabled**, not `primary`: `Discard` was passing `false` and so could not be
    // pressed at all, which left a new ticket with no way to be thrown away from its own editor.
    let buttons: &[(&str, bool)] = match new {
        true => &[("Discard", true), ("Done", true)],
        false => &[("Close", true)],
    };
    // **`Confirm::CommandEnter`, because this modal's body owns `Enter`.** The comment here used to say
    // that and the code said the opposite: `modal::footer` *is* the `Confirm::Enter` one, so pressing
    // `Enter` while typing a description or a comment closed the ticket — in the same frame as posting
    // the comment, so the words went in and the modal went away. It is the commit panel's exception,
    // reached for the same reason: a multiline field is a field where `Enter` is a new line, and a new
    // line is what a person pressing it there means. `Escape` still closes it, which `modal::show` owns
    // and every dialog in Quill shares.
    if let Some(pressed) = modal::footer_confirmed_by(ui, footer, buttons, modal::Confirm::CommandEnter) {
        match (new, pressed) {
            (true, 0) => match board.discard_the_ticket() {
                Ok(()) => outcome.closed = true,
                Err(problem) => outcome.requests.push(Request::Message(problem)),
            },
            _ => outcome.closed = true,
        }
    }
    // What the footer says beside its buttons, which is the browser's own sentence.
    if new {
        text(
            ui.painter(),
            Pos2::new(footer.min.x + PAD, footer.center().y - 6.0),
            "Starts saving as you type",
            look.font_size - 1.5,
            look.palette.text_faint,
        );
    }
    outcome
}

/// The description, the todos, the terminal and the comments.
///
/// ## The heights add up, and that is the whole of this function's difficulty
///
/// The four sections are laid out one under another with no scroll, so a budget that overflows does not
/// clip — it draws the last section off the bottom edge and the one before it over its own buttons, which is
/// what `task-1771`'s capture of the modal shows. The description used to be given "whatever is left, floored
/// at ninety", and a floor is exactly the thing that makes a budget stop adding up.
///
/// So the room is shared out the other way round: every section says what it **wants** and what it can be cut
/// to, and the shortfall is taken from them in order — the terminal first, then the comments, then the todos,
/// and the description last, because a ticket is opened to write in far more often than to read an agent's
/// scrollback. Whatever is left over goes to the description, which is the one section that can use it.
fn left_column(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
    task: &Task,
    new: bool,
) -> Vec<Request> {
    let mut requests = Vec::new();
    let scale = look.scale();
    let mut pen = area.min.y;
    let heading = look.font_size + 6.0;
    let gap = 12.0;

    // The title, which is in the header on a ticket that exists — see `contents`. A **new** one has no title
    // yet and this is where it is typed, because a header is not a field.
    if new {
        let title_at = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(area.width(), 30.0));
        let mut title = board.detail().title_draft.clone();
        let response = ui.put(
            crate::components::controls::field_text_rect(ui, title_at, 2.0),
            egui::TextEdit::singleline(&mut title)
                .frame(egui::Frame::NONE)
                .hint_text(egui::RichText::new("What needs doing?").color(look.palette.text_faint))
                .desired_width(area.width())
                .font(egui::FontId::proportional(look.font_size + 4.0))
                .text_color(look.palette.text_strong),
        );
        if response.changed() {
            board.detail_mut().title_draft = title;
            if let Err(problem) = board.save_the_title() {
                requests.push(Request::Message(problem));
            }
        }
        pen = title_at.max.y + 8.0;
    }

    let todos_open = !board.todos_shut;
    let terminal_open = !board.terminal_shut;
    let room = area.max.y - pen;

    // What each section wants, and the least it can be given. A section that is shut wants its heading and
    // nothing else, which is what makes shutting one worth doing.
    let (todo_want, todo_least) = match (new, todos_open) {
        (true, _) | (_, false) => (0.0, 0.0),
        _ => {
            let rows = board.detail().todos.len() as f32;
            let wanted = rows * look.row_height + look.row_height + 8.0;
            (wanted.min(160.0 * scale), look.row_height * 2.0)
        }
    };
    let (terminal_want, terminal_least) = match (new, terminal_open) {
        (true, _) | (_, false) => (0.0, 0.0),
        _ => ((room * TERMINAL_SHARE).clamp(TERMINAL_SMALLEST * scale, TERMINAL_LARGEST * scale), 90.0 * scale),
    };
    let (comment_want, comment_least) = match new {
        true => (0.0, 0.0),
        false => (COMMENTS_AT_DEFAULT * scale, 96.0 * scale),
    };
    // The headings and the gaps between the sections, which are room nothing else can have.
    let headings = match new {
        true => heading + gap,
        false => heading * 4.0 + gap * 4.0,
    };
    let description_want = (room - headings - todo_want - terminal_want - comment_want).max(0.0);
    let description_least = 90.0 * scale;

    // Take the shortfall from the sections in order, each down to its own least.
    let mut short =
        (headings + description_want.max(description_least) + todo_want + terminal_want + comment_want - room)
            .max(0.0);
    let give = |want: f32, least: f32, short: &mut f32| -> f32 {
        let spare = (want - least).max(0.0).min(*short);
        *short -= spare;
        want - spare
    };
    let terminal_height = give(terminal_want, terminal_least, &mut short);
    let comment_height = give(comment_want, comment_least, &mut short);
    let todo_height = give(todo_want, todo_least, &mut short);
    let description_height = give(description_want.max(description_least), description_least, &mut short);
    // **And whatever is still short comes off the description**, which is the only section that can be
    // drawn small and still be a section: the todos are rows, the terminal is a character grid and the
    // comments are a list with a box under them, and each has a size below which it is a strip. A modal
    // dragged down to `modal::MIN_HEIGHT` has less room than every minimum added up, and a budget that
    // stopped at "every section is at its least" would still have run off the bottom. Found by the
    // `task-1771` review.
    let description_height = (description_height - short).max(0.0);

    label(ui, look, Pos2::new(area.min.x, pen), "Description");
    // The two view buttons, on the label's own row and right aligned, which is where a section's controls go.
    // `task-28`.
    if let Some(rendered) = super::raw_or_rendered(
        ui,
        look,
        Rect::from_min_size(Pos2::new(area.min.x, pen - 3.0), Vec2::new(area.width(), 18.0)),
        "the description",
        board.detail().description_rendered,
    ) {
        board.show_the_description_rendered(rendered);
    }
    pen += heading;
    let description_at =
        Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(area.width(), description_height));
    // **The description sits in a well**, which is what the reference draws and what a board in dark
    // neumorphism means by a field. Behind the editor rather than round it, so the caret, the selection and
    // the syntax colouring are unchanged.
    if look.chrome.is_recording() {
        look.chrome.sunken(
            description_at,
            look.corner_radius + 4.0,
            look.ground(look.palette.board_well),
            crate::services::vello_canvas::Lift::Small,
        );
    }
    requests.extend(super::description::show(board, ui, description_at, look));
    pen = description_at.max.y + gap;

    if new {
        return requests;
    }

    // **Todos and the terminal fold**, which is what the page this is modelled on does and what makes a
    // ticket with a long conversation on it readable at all. The flags are the provider's, so a section left
    // shut stays shut while the board is refreshed under it.
    if disclosure(
        ui,
        look,
        Pos2::new(area.min.x, pen),
        &format!("Todos \u{b7} {}/{}", task.todo_done_count, task.todo_count),
        !todos_open,
    ) {
        board.todos_shut = todos_open;
    }
    pen += heading;
    if todos_open {
        let todos_at = Rect::from_min_size(
            Pos2::new(area.min.x, pen),
            Vec2::new(area.width(), todo_height.min((area.max.y - pen).max(0.0))),
        );
        requests.extend(super::detail::todo_rows(board, ui, todos_at, look));
        pen = todos_at.max.y + gap;
    }

    let attached =
        board.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running());
    let said = match attached {
        true => format!("Agent terminal \u{b7} live \u{b7} {}", task.key),
        false => "Agent terminal".to_owned(),
    };
    if disclosure(ui, look, Pos2::new(area.min.x, pen), &said, !terminal_open) {
        board.terminal_shut = terminal_open;
    }
    // **No second `Resume session` here.** The one button at the top of the right column already becomes
    // it when a ticket has a session and no terminal, and a copy beside this heading was a second control
    // with the same plain name — which `choice_button` also derives its id from, so the two shared that as
    // well. Found by the `task-1771` review; the rule is `CLAUDE.md`'s "give every control a name", and two
    // controls with one name is the case it exists to stop.
    pen += heading;
    if terminal_open {
        let terminal_at = Rect::from_min_size(
            Pos2::new(area.min.x, pen),
            Vec2::new(area.width(), terminal_height.min((area.max.y - pen).max(0.0))),
        );
        if terminal_at.height() > 20.0 {
            requests.extend(super::detail::terminal_section(board, ui, terminal_at, look, task, false));
        }
        pen = terminal_at.max.y + gap;
    }

    label(
        ui,
        look,
        Pos2::new(area.min.x, pen),
        &format!("Comments \u{b7} {}", task.comment_count),
    );
    pen += heading;
    let comments_at = Rect::from_min_size(
        Pos2::new(area.min.x, pen),
        Vec2::new(area.width(), comment_height.min((area.max.y - pen).max(0.0))),
    );
    if comments_at.height() > 20.0 {
        requests.extend(super::detail::comment_section(board, ui, comments_at, look));
    }
    requests
}

/// Everything that is a property of the ticket rather than its contents.
///
/// **In the order the page this is modelled on has them**, which is not the order they were in: the one
/// button somebody opens a ticket to press is at the **top**, then the seven things about the ticket, then
/// when it was made, and `Delete task` last and in red. `task-1771` reports this column as looking like
/// nobody had arranged it, and an arrangement is what an order is.
fn right_column(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
    task: &Task,
    new: bool,
) -> Vec<Request> {
    // **It scrolls, because it cannot be made to fit.** A button, seven fields, two lines of prose, a JIRA
    // key, a date and a Delete are about 570 points at the default size — more than the column has in a
    // modal dragged down towards its smallest, and far more than the 330 points the one-column layout gives
    // it. Nothing here can be dropped: every one of them is a thing a ticket needs before an agent can be
    // started, which is what `task-28` added them for. So the room is what it is and the column scrolls,
    // which is what the page this is modelled on does with the whole of its own body. Found by the
    // `task-1771` review, which measured the overlap.
    let mut requests = Vec::new();
    let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(area));
    inside.set_clip_rect(area);
    egui::ScrollArea::vertical()
        .id_salt("agent-tasks-ticket-fields")
        .auto_shrink([false, false])
        .show(&mut inside, |ui| {
            let top = ui.cursor().min.y;
            let at = Rect::from_min_size(
                Pos2::new(area.min.x, top),
                Vec2::new(area.width(), area.height().max(1.0)),
            );
            let (asked, used) = fields(board, ui, at, look, task, new);
            requests = asked;
            // What the scrollbar measures itself against. Allocated rather than left to the widgets, none of
            // which allocate at all: every one of them is drawn at a rectangle this function worked out.
            ui.allocate_space(Vec2::new(area.width(), (used - top).max(0.0)));
        });
    requests
}

/// The fields themselves, answering where the last of them ended.
fn fields(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
    task: &Task,
    new: bool,
) -> (Vec<Request>, f32) {
    let mut requests = Vec::new();
    let mut pen = area.min.y;
    let width = area.width() - PAD;
    let field = |pen: f32| Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(width, FIELD));

    // ---------------------------------------------------------------- the one thing to press
    //
    // At the top, which is where the reference puts it and where somebody opening a ticket to start an agent
    // looks first. It used to be eight fields down, under `Created just now`. Absent when it cannot apply,
    // which is Quill's rule: a ticket with an agent already running offers `Stop` instead, and a new one
    // offers nothing at all because it has no title yet.
    if !new {
        let attached =
            board.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running());
        let (label, command) = match () {
            _ if attached => ("Stop", "stop"),
            _ if task.session_id.is_none() => ("Start Work", "start"),
            _ if crate::services::agent_tasks::agent::can_resume(task.assignee) => {
                ("Resume session", "resume")
            }
            // **`Start Work again`, not `Resume session`.** Codex names its own sessions, so the id on a
            // Codex ticket is only Quill's marker that a worker was here and there is no conversation to hand
            // back. The label says `again` because it is a new conversation, and the comments are what the
            // new agent reads.
            _ => ("Start Work again", "start"),
        };
        let at = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(width, 34.0));
        if super::primary_button(ui, look, at, label) {
            match board.command_now(command, std::slice::from_ref(&task.key)) {
                Ok(answer) if !answer.message.is_empty() => {
                    requests.push(Request::Message(answer.message))
                }
                Ok(_) => {}
                Err(problem) => requests.push(Request::Message(problem)),
            }
        }
        pen += 34.0 + 10.0;
    }

    // ---------------------------------------------------------------- what the ticket is
    //
    // The lane, which is what `Status` is in the browser. Absent for a new ticket: it is in New and moving it
    // before it has a title is not a thing anybody wants.
    if !new {
        let (chosen, tall) = dropdown_row(
            ui,
            look,
            field(pen),
            "Status",
            &Status::ALL.map(|status| (status.name().to_owned(), status.label().to_owned())),
            task.status.name(),
            None,
        );
        if let Some(chosen) = chosen {
            if let Some(status) = Status::parse(&chosen) {
                if let Err(problem) = board.move_card(task.id, status, i64::MAX) {
                    requests.push(Request::Message(problem));
                }
            }
        }
        pen += tall + 4.0;
    }

    let (chosen, tall) = dropdown_row(
        ui,
        look,
        field(pen),
        "Assignee",
        &Assignee::ALL.map(|assignee| (assignee.name().to_owned(), assignee.name().to_owned())),
        task.assignee.name(),
        None,
    );
    if let Some(chosen) = chosen {
        requests.extend(write(board, task, Field::Assignee(chosen)));
    }
    pen += tall + 4.0;

    // **Absent** for a ticket assigned to a person rather than disabled, which is Quill's rule: the `F` button
    // is not drawn for a `.rs` file either.
    if task.assignee.is_an_agent() {
        // **A dropdown, not a text field.** `task-28`: an agent could not be started because a model
        // identifier had to be typed from memory. `agent::models_for` keeps whatever the row already says in
        // the list, so opening a ticket in a dropdown cannot change which model it names.
        let model = task.model.clone().unwrap_or_default();
        let models: Vec<(String, String)> =
            crate::services::agent_tasks::agent::models_for(task.assignee, task.model.as_deref())
                .into_iter()
                .map(|name| (name.clone(), name))
                .collect();
        let (chosen, tall) =
            dropdown_row(ui, look, field(pen), "Model", &models, &model, Some("the agent's default"));
        if let Some(chosen) = chosen {
            requests.extend(write(board, task, Field::Model(chosen)));
        }
        pen += tall + 4.0;

        let (chosen, tall) = dropdown_row(
            ui,
            look,
            field(pen),
            "Effort",
            &EFFORTS.iter().map(|level| ((*level).to_owned(), (*level).to_owned())).collect::<Vec<_>>(),
            task.effort.as_deref().unwrap_or(""),
            Some("Model default"),
        );
        if let Some(chosen) = chosen {
            requests.extend(write(board, task, Field::Effort(chosen)));
        }
        pen += tall;
        pen += helper(ui.painter(), look, Pos2::new(area.min.x, pen), "Reasoning depth the agent CLI runs at");
        pen += 6.0;
    }

    // The projects this window knows about, which is the list `File -> Open Recent` draws: a folder somebody
    // has opened is a folder they might point a ticket at. A ticket may still name one this window has never
    // opened, so whatever the row says is kept in the list the way a model is.
    let project = task.project.clone().unwrap_or_default();
    let projects: Vec<(String, String)> = board
        .known_projects(task.project.as_deref())
        .into_iter()
        .map(|path| (path.clone(), path))
        .collect();
    let (chosen, tall) = dropdown_row(
        ui,
        look,
        field(pen),
        "Project",
        &projects,
        &project,
        Some("the folder this window has open"),
    );
    if let Some(chosen) = chosen {
        requests.extend(write(board, task, Field::Project(chosen)));
    }
    pen += tall;
    pen += helper(ui.painter(), look, Pos2::new(area.min.x, pen), "Repo the agent terminal opens in");
    pen += 6.0;

    let (chosen, tall) = dropdown_row(
        ui,
        look,
        field(pen),
        "Priority",
        &Priority::ALL.map(|priority| (priority.name().to_owned(), priority.name().to_owned())),
        task.priority.name(),
        None,
    );
    if let Some(chosen) = chosen {
        requests.extend(write(board, task, Field::Priority(chosen)));
    }
    pen += tall + 4.0;

    let epics: Vec<(String, String)> =
        board.board().epics.iter().map(|epic| (epic.id.to_string(), epic.name.clone())).collect();
    let (chosen, tall) = dropdown_row(
        ui,
        look,
        field(pen),
        "Epic",
        &epics,
        &task.epic_id.map(|id| id.to_string()).unwrap_or_default(),
        Some("None"),
    );
    if let Some(chosen) = chosen {
        requests.extend(write(board, task, Field::Epic(chosen)));
    }
    pen += tall + 4.0;

    // ---------------------------------------------------------------- the JIRA issue, and when it was made
    //
    // **What it does not do is sync.** There is no HTTP client in Quill, which
    // `tasks/agent-tasks-plugin-tdd.md` §10 records, so the key is a field somebody types rather than one a
    // sync brought in. Copy hands over the row's own `jira_url` when it has one and the key otherwise,
    // because there is no configured JIRA site to build an address against and a guessed address that opens
    // nothing is worse than the key.
    //
    // Below the seven fields rather than above them, which is the one place this column departs from the
    // reference's order and the reason is the reference's own: the browser draws this panel only on a ticket
    // that came from JIRA, and with no sync a panel that appeared only once a key was set could never be the
    // thing that set one. So it is drawn on every ticket, and put where a field nobody has filled in belongs.
    if !new {
        let key = task.jira_key.clone().unwrap_or_default();
        let (typed, tall) = field_row(ui, look, field(pen), "JIRA", &key, "no issue");
        if let Some(typed) = typed {
            requests.extend(write(board, task, Field::JiraKey(typed)));
        }
        pen += tall;
        // Copy only when there is something to copy, which is Quill's rule about a control that cannot apply.
        if !key.trim().is_empty() {
            let copy = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(width.min(130.0), 20.0));
            if crate::components::controls::choice_button(ui, copy, "Copy issue link", false) {
                requests.push(Request::Copy(board.jira_link(&key)));
                requests.push(Request::Message(format!("copied the link to {key}")));
            }
            // What a sync would have filled in, when a row carries it: a ticket that came from JIRA has its
            // issue type and the status JIRA itself holds, and neither is a thing this board can change.
            let said = [task.jira_issue_type.clone(), task.jira_status.clone()]
                .into_iter()
                .flatten()
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<String>>()
                .join(" \u{b7} ");
            if !said.is_empty() {
                text(
                    ui.painter(),
                    Pos2::new(copy.max.x + 8.0, pen + 3.0),
                    &said,
                    look.font_size - 2.5,
                    look.palette.text_faint,
                );
            }
            pen += 26.0;
        }
        pen += 4.0;

        let now = clock::now();
        paint_label(ui.painter(), look, Pos2::new(area.min.x, pen), "Created");
        pen += look.font_size;
        text(
            ui.painter(),
            Pos2::new(area.min.x, pen),
            &clock::relative(&task.created_at, &now),
            look.font_size - 1.0,
            look.palette.text_dim,
        );
        pen += look.font_size + 14.0;

        // ------------------------------------------------------------ and the one thing that destroys work
        //
        // Last, in the one red the board already has, with the bin beside it — which is where and how the
        // reference draws it. Pressed once it says what it will do, pressed twice it does it: the browser
        // board asks with a `confirm()`, and a second press is the smaller answer that fits in a column.
        // Deleting a ticket takes its todos and its comments with it, so it is the one control here that asks.
        let asking = board.delete_asked;
        let said = match asking {
            true => "Delete for good",
            false => "Delete task",
        };
        let at = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(width, 24.0));
        if danger_button(ui, look, at, said) {
            match asking {
                true => {
                    board.delete_asked = false;
                    if let Err(problem) = board.discard_the_ticket() {
                        requests.push(Request::Message(problem));
                    }
                }
                false => board.delete_asked = true,
            }
        }
        pen += 28.0;
        if asking {
            let at = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(width, 22.0));
            if crate::components::controls::choice_button(ui, at, "Keep it", false) {
                board.delete_asked = false;
            }
            pen += 26.0;
        }
    }
    (requests, pen)
}

/// The one control on a ticket that destroys work: a bin, a word, and the board's own red.
///
/// Not a button with a filled ground. The reference draws it as a red row rather than a red button, and that
/// is a real distinction rather than a decorative one: a filled button among seven quiet fields reads as the
/// thing to press, and this is the thing not to press.
fn danger_button(ui: &mut egui::Ui, look: &Look<'_>, area: Rect, said: &str) -> bool {
    let tint = crate::theme::color::CLOSE;
    let response =
        ui.interact(area, ui.id().with(("agent-tasks-danger", said)), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            area,
            CornerRadius::same((look.corner_radius + 2.0) as u8),
            tint.gamma_multiply(0.12),
        );
    }
    let galley = ui.painter().layout_no_wrap(
        said.to_owned(),
        egui::FontId::proportional(look.font_size - 1.0),
        tint,
    );
    let bin = Pos2::new(area.center().x - galley.size().x / 2.0 - 12.0, area.center().y);
    crate::theme::icon::bin(ui.painter(), bin, tint);
    ui.painter().galley(
        Pos2::new(area.center().x - galley.size().x / 2.0, area.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), said)
    });
    response.clicked()
}

/// Write one field, and report what could not be written.
fn write(board: &mut AgentTasks, task: &Task, field: Field) -> Vec<Request> {
    match board.edit_field(task.id, field) {
        Ok(()) => Vec::new(),
        Err(problem) => vec![Request::Message(problem)],
    }
}

/// A section's or a field's name, drawn and **named**.
///
/// Named because `CLAUDE.md` asks that every control have a plain name a test can find it by, and a heading
/// over a field is what tells a person and a test which field they are looking at. Painted text alone is
/// invisible to both.
/// A section's heading that can be pressed to shut the section, and says which state it is in.
///
/// The triangle is the disclosure every tree in Quill draws, and the whole heading is the target rather than
/// only the triangle, because a heading is easier to hit than an eight point mark. Answers whether it was
/// pressed; the caller flips its own flag, because the flag lives on the provider and this draws.
fn disclosure(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    at: Pos2,
    said: &str,
    shut: bool,
) -> bool {
    let painter = ui.painter().clone();
    let middle = at.y + look.font_size / 2.0;
    let mark = 4.0;
    let tint = look.palette.text_dim;
    match shut {
        // Pointing right when shut and down when open, which is what the explorer's folders do.
        true => painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(at.x, middle - mark),
                Pos2::new(at.x + mark * 1.4, middle),
                Pos2::new(at.x, middle + mark),
            ],
            tint,
            egui::Stroke::NONE,
        )),
        false => painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(at.x - mark, middle - mark / 2.0),
                Pos2::new(at.x + mark, middle - mark / 2.0),
                Pos2::new(at.x, middle + mark),
            ],
            tint,
            egui::Stroke::NONE,
        )),
    };
    // Painted the way every other section's name on this ticket is painted - upper case, spaced and quiet -
    // and **named** in the case a person reads, which is the same split `label` makes. A test and an agent
    // ask for `Todos`; the drawing is what shouts.
    let words = Pos2::new(at.x + 12.0, at.y);
    let width = paint_label(&painter, look, words, said);
    let area = Rect::from_min_size(at, Vec2::new(width + 14.0, look.font_size + 2.0));
    let response = ui.interact(
        area,
        ui.id().with(("agent-tasks-disclosure", said)),
        egui::Sense::click(),
    );
    let name = match shut {
        true => format!("{said}, shut"),
        false => format!("{said}, open"),
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), !shut, name.clone())
    });
    response.clicked()
}

fn label(ui: &mut egui::Ui, look: &Look<'_>, at: Pos2, said: &str) {
    let width = paint_label(ui.painter(), look, at, said);
    let area = Rect::from_min_size(at, Vec2::new(width, look.font_size));
    let response = ui.interact(area, ui.id().with(("agent-tasks-label", said)), egui::Sense::hover());
    let name = said.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name.clone()));
}

/// A section's or a field's name as the reference draws one: small, quiet, upper case and letter spaced.
///
/// **Spaced by hand**, because `egui` has no letter spacing setting — the explorer's own heading does exactly
/// this for exactly this reason. The tracking is what makes a run of capitals read as a label rather than as
/// shouting, and it is the one thing that turns eight fields down a column into a form.
fn paint_label(painter: &egui::Painter, look: &Look<'_>, at: Pos2, said: &str) -> f32 {
    let spaced: String = said.to_uppercase().chars().flat_map(|letter| [letter, '\u{2009}']).collect();
    text(painter, at, spaced.trim_end(), look.font_size - 3.5, look.palette.text_faint)
}

/// A quiet line of prose under a field, saying what it is for. `EFFORT` and `PROJECT` both have one on the
/// page this is modelled on, and they are the two fields whose names do not say what they do.
fn helper(painter: &egui::Painter, look: &Look<'_>, at: Pos2, said: &str) -> f32 {
    text(painter, at, said, look.font_size - 3.0, look.palette.text_faint);
    look.font_size - 1.0
}

/// A named value chosen from a list, answering what was chosen when it changed.
///
/// `task-28`: "Dropdowns. We need UI dropdowns for values." Every one of the ticket's fields that holds one of
/// a known set is this, and there is one of these rather than seven arrangements of buttons and boxes.
///
/// `components::controls::dropdown` is what draws it, which is the control the toolbar and
/// `Settings -> Appearance` already use, so a dropdown on a ticket opens and closes and looks like every other
/// dropdown in Quill. `options` is `(value, said)` pairs — the value written to the row and the words a person
/// reads — which is the shape `choice_row` took before this, so the call sites did not have to change shape.
///
/// `empty` is what the list calls holding nothing, for a field that may. `None` means the field is required and
/// the list offers no way to clear it.
fn dropdown_row(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    name: &str,
    options: &[(String, String)],
    chosen: &str,
    empty: Option<&str>,
) -> (Option<String>, f32) {
    // **Painted rather than named.** `label` registers a `Label` in the accessibility tree, and the dropdown
    // below carries the same name — which is the pairing a person wants and two nodes with one name, so a test
    // asking for `Model` could not tell which it had. The control is the one that answers to the name; the words
    // above it are the words above it.
    paint_label(ui.painter(), look, area.min, name);
    let at = Rect::from_min_size(
        Pos2::new(area.min.x, area.min.y + look.font_size + 2.0),
        Vec2::new(area.width(), 26.0),
    );
    // **The well the value sits in is the board's own.** `controls::dropdown` draws a flat field, which is
    // right everywhere else in Quill and wrong here: this modal is on a board drawn in dark neumorphism, and
    // a flat box among raised cards is the "plain, not much effort" `task-1771` reports. Painted before the
    // dropdown, so the dropdown's own text and chevron land on top of it.
    if look.chrome.is_recording() {
        look.chrome.sunken(
            at,
            look.corner_radius + 2.0,
            look.ground(look.palette.board_well),
            crate::services::vello_canvas::Lift::Small,
        );
    }
    let picked =
        super::value_dropdown_over(ui, at, name, options, chosen, empty, !look.chrome.is_recording());
    (picked, look.font_size + 30.0)
}

/// A named field, answering what was typed when it changed.
fn field_row(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    name: &str,
    value: &str,
    hint: &str,
) -> (Option<String>, f32) {
    label(ui, look, area.min, name);
    let at = Rect::from_min_size(
        Pos2::new(area.min.x, area.min.y + look.font_size + 2.0),
        Vec2::new(area.width(), 24.0),
    );
    // The same well every value on this column sits in, so the one field among eight dropdowns does not read
    // as a different kind of control. See `dropdown_row`.
    if look.chrome.is_recording() {
        look.chrome.sunken(
            at,
            look.corner_radius + 2.0,
            look.ground(look.palette.board_well),
            crate::services::vello_canvas::Lift::Small,
        );
    } else {
        ui.painter().rect(
            at,
            CornerRadius::same(look.corner_radius as u8),
            look.palette.field,
            egui::Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let mut typed = value.to_owned();
    // Its own id scope for the reason a row of choices has one: two fields whose hint happens to match would be
    // two text boxes sharing an id.
    let changed = ui
        .push_id(name, |ui| {
            let response = ui.put(
                crate::components::controls::field_text_rect(ui, at, 6.0),
                egui::TextEdit::singleline(&mut typed)
                    .frame(egui::Frame::NONE)
                    .hint_text(egui::RichText::new(hint).color(look.palette.text_faint))
                    .font(egui::FontId::proportional(look.font_size - 1.0))
                    .text_color(look.palette.text),
            );
            response.changed()
        })
        .inner;
    (changed.then_some(typed), look.font_size + 30.0)
}
