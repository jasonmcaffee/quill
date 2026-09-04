//! One ticket: its title, its description, its todos, its comments and its terminal.
//!
//! The description is markdown and Unluminous already reads markdown, so it is drawn by the same reader the
//! Markdown preview uses rather than by a second one written here. That is the largest single saving in
//! the plugin and it falls out of the plugin being inside a text editor.

use egui::{CornerRadius, Pos2, Rect, Sense, Vec2};

use super::{clipped, text};
use crate::services::agent_tasks::{clock, AgentTasks};
use crate::services::plugin_ui::{Look, Request};
use crate::theme::icon;

// **The in-place ticket is gone.** This file used to draw a whole ticket inside the board's own rectangle —
// the pane's narrow column, and the right hand half of the tab — and `task-1771` asked for that to stop: a
// ticket is the modal and nothing else, so a board that split itself in two the moment an agent read a
// ticket is a board that rearranged itself under somebody's hands. `show` and `todos_and_comments` went
// with it, along with the three measurements only they used. What is left is what the modal lays out, which
// was always shared between the two and is now called from one place.

/// The ticket's own terminal: the real one.
///
/// `components::terminal_panel::grid` is what the terminal tile and the run tile are both made of, so this is
/// the same emulator, the same colours, the same selection, the same clipboard rules and **the keyboard into
/// the program**, which is the whole of what the ticket meant by terminal chat: an agent asking a question
/// deserves an answer typed at it. Painting a picture of a terminal instead, which is what this was, gave a
/// board that could watch an agent and not talk to it.
pub(crate) fn terminal(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    task_id: i64,
    focused: bool,
) -> Vec<Request> {
    let painter = ui.painter().clone();
    painter.rect_filled(
        Rect::from_min_size(area.min, Vec2::new(area.width(), 1.0)),
        0,
        look.palette.divider,
    );
    let grid_area = Rect::from_min_max(Pos2::new(area.min.x, area.min.y + 1.0), area.max);
    let monospace = look.monospace_size;
    let opacity = look.opacity;
    let renderer = look.renderer;
    let mut selecting = board.terminal_selecting;
    let outcome = {
        let session = board.terminal_for_mut(task_id).map(|terminal| &mut terminal.session);
        crate::components::terminal_panel::grid(
            ui,
            grid_area,
            session,
            &mut selecting,
            focused,
            "agent-tasks-terminal",
            "No terminal for this ticket. Press Start to launch its agent, or Resume session to hand its \
             conversation back.",
            renderer,
            monospace,
            opacity,
        )
    };
    board.terminal_selecting = selecting;
    let mut requests: Vec<Request> = Vec::new();
    if outcome.take_focus {
        // Both halves, because they are two different things. `UnluminousApp::focus` is the one value that says who in
        // the window has the keyboard, and a plugin that set only its own flag left the editing area holding the
        // keys as well, so one press reached both. And this flag is which part of the **board** the keys go to,
        // which the window cannot know: it hands the keyboard over the same way when somebody clicks the lanes.
        requests.push(Request::TakeTheKeyboard(true));
        board.focus_the_terminal(true);
    }
    if let Some(text) = outcome.copy {
        requests.push(Request::Copy(text));
    }
    // No repaint asked for here. The session has the window's own waker, so it asks for a frame when it prints,
    // and a terminal that is alive and quiet needs none: asking every frame while an agent sits at its prompt
    // was the window drawing for ever for nothing.
    requests
}

/// The todo rows on their own, for the modal, which lays its sections out itself.
///
/// The same rows the pane draws, so a todo ticked in one place is a todo ticked in the other: this is the one
/// function that draws them and the pane and the modal both call it.
pub(crate) fn todo_rows(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
) -> Vec<Request> {
    let mut requests = Vec::new();
    let painter = ui.painter().clone();
    let todos = board.detail().todos.clone();
    // How far the list is scrolled, before it is drawn: the wheel over this section moves the todos rather than
    // the lane behind them.
    let content = todos.len() as f32 * look.row_height;
    let room = area.height() - look.row_height;
    board.scroll_the_todos(ui, area, (content - room).max(0.0));
    let mut pen = area.min.y - board.todo_scroll;
    let mut toggled = None;
    let mut removed = None;
    for todo in &todos {
        // Scrolled rather than stopped: a hundred todos used to draw as many as fit and then nothing, so a todo
        // past the fold could not be ticked or removed.
        if pen + look.row_height < area.min.y {
            pen += look.row_height;
            continue;
        }
        if pen + look.row_height > area.max.y - look.row_height {
            break;
        }
        let box_at = Rect::from_min_size(Pos2::new(area.min.x, pen + 4.0), Vec2::splat(14.0));
        let response = ui.interact(
            box_at.expand(3.0),
            ui.id().with(("agent-tasks-modal-todo", todo.id)),
            Sense::click(),
        );
        // Named as a tick box, for the reason the pane's todos are: see `todo_section`.
        let said = todo.text.clone();
        let ticked = todo.done;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), ticked, said.clone())
        });
        painter.rect(
            box_at,
            CornerRadius::same(3),
            match todo.done {
                true => look.palette.accent,
                false => look.palette.field,
            },
            egui::Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
        if todo.done {
            icon::tick(&painter, box_at.center(), look.palette.text_strong);
        }
        if response.clicked() {
            toggled = Some((todo.id, !todo.done));
        }
        let tint = match todo.done {
            true => look.palette.text_dim,
            false => look.palette.text,
        };
        clipped(
            &painter,
            Pos2::new(area.min.x + 22.0, pen + 2.0),
            &todo.text,
            look.font_size - 0.5,
            tint,
            area.width() - 52.0,
            1,
        );
        // A cross that removes it. The browser's todos are read only because the agent writes its own plan; here
        // they can be written, and a list that can be added to and not removed from is half a list.
        let cross = Rect::from_center_size(
            Pos2::new(area.max.x - 12.0, pen + look.row_height / 2.0 - 2.0),
            Vec2::splat(18.0),
        );
        if crate::components::controls::icon_button(ui, cross, &format!("Remove {}", todo.text), icon::cross) {
            removed = Some(todo.id);
        }
        pen += look.row_height;
    }
    if let Some((id, done)) = toggled {
        if let Err(problem) = board.set_todo(id, done) {
            requests.push(Request::Message(problem));
        }
    }
    if let Some(id) = removed {
        if let Err(problem) = board.remove_the_todo(id) {
            requests.push(Request::Message(problem));
        }
    }
    // The box that adds one, at the foot of the section.
    let at = Rect::from_min_size(
        Pos2::new(area.min.x, area.max.y - look.row_height),
        Vec2::new(area.width(), look.row_height),
    );
    painter.rect(
        at,
        CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    let mut draft = board.detail().todo_draft.clone();
    let todo_id = ui.id().with("agent-tasks-todo-draft");
    let response = ui.put(
        crate::components::controls::field_takes_the_whole_rectangle(ui, at, 8.0, todo_id),
        egui::TextEdit::singleline(&mut draft)
            .id(todo_id)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new("Add a todo").color(look.palette.text_faint))
            .font(egui::FontId::proportional(look.font_size - 0.5))
            .text_color(look.palette.text),
    );
    if response.changed() {
        board.detail_mut().todo_draft = draft;
    }
    if super::enter_was_used_and_pressed(ui, &response) {
        if let Err(problem) = board.post_the_todo() {
            requests.push(Request::Message(problem));
        }
        response.request_focus();
    }
    requests
}

/// The terminal, with the header the browser board puts over it: the word, whether it is attached, and the
/// button that hands the conversation back when it is not.
/// `heading` says whether to draw a row naming the section. The ticket modal draws its own — a disclosure
/// that folds the terminal away, with the ticket's key and whether it is live in it — so it asks for none,
/// and the grid takes the whole rectangle. `task-1771`: one heading, not two under each other.
pub(crate) fn terminal_section(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
    task: &crate::services::agent_tasks::model::Task,
    heading: bool,
) -> Vec<Request> {
    let mut requests = Vec::new();
    let painter = ui.painter().clone();
    let attached = board.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running());
    let mut top = area.min.y;
    if heading {
        let head = Rect::from_min_size(area.min, Vec2::new(area.width(), 22.0));
        let mut pen = head.min.x;
        pen += text(
            &painter,
            Pos2::new(pen, head.min.y),
            "Terminal",
            look.font_size - 1.5,
            look.palette.text_dim,
        );
        let (said, tint) = match attached {
            true => ("attached", look.palette.added),
            false => ("detached", look.palette.text_faint),
        };
        text(&painter, Pos2::new(pen + 8.0, head.min.y), said, look.font_size - 2.0, tint);
        // Only a ticket that already has a session gets a button here. Starting an agent is Start work's
        // job, so there is no second control that does it.
        if !attached && task.session_id.is_some() {
            let at =
                Rect::from_min_size(Pos2::new(area.max.x - 110.0, head.min.y - 3.0), Vec2::new(110.0, 22.0));
            if crate::components::controls::choice_button(ui, at, "Resume session", false) {
                match board.command_now("resume", std::slice::from_ref(&task.key)) {
                    Ok(answer) if !answer.message.is_empty() => {
                        requests.push(Request::Message(answer.message))
                    }
                    Ok(_) => {}
                    Err(problem) => requests.push(Request::Message(problem)),
                }
            }
        }
        top = head.max.y + 2.0;
    }
    let grid = Rect::from_min_max(Pos2::new(area.min.x, top), area.max);
    requests.extend(terminal(board, ui, look, grid, task.id, board.terminal_focused));
    requests
}

/// The comments and the box that posts one, for the modal.
pub(crate) fn comment_section(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    area: Rect,
    look: &Look<'_>,
) -> Vec<Request> {
    let mut requests = Vec::new();
    let painter = ui.painter().clone();
    let comments = board.detail().comments.clone();
    let box_height = look.row_height + 30.0;
    // **No count line.** The heading above this section carries it — `COMMENTS \u{b7} 3` — and a section that
    // said how many comments it held immediately under a heading that said the same thing was one fact drawn
    // twice, in a column where every point of height is being argued over. `task-1771`.
    let pen = area.min.y;
    let room_for_comments = area.max.y - box_height - 6.0;
    let editing = board.detail().editing_comment;
    let mut edited = board.detail().comment_edit.clone();
    // Which comments are being read as their source, and the cache the rendered ones are drawn from. Taken out of
    // `board` before the closure below, because `board.markdown` is borrowed for the whole of it while
    // `board.detail()` would be borrowed again inside — two fields of one struct, so they are named separately.
    let raw_comments = board.detail().comments_raw.clone();
    let now = clock::now();
    // **Newest first, and scrolled**, which is two faults gone. The modal drew them oldest first and simply
    // stopped when it ran out of room, so on a ticket with a real conversation on it the newest comment — the one
    // somebody opened the ticket to read — was the one that could not be seen, and there was no way to reach it.
    // Newest first also makes the modal agree with the pane, which already ordered them this way: switching from
    // one to the other used to reverse the conversation.
    //
    // The scroll is a real `ScrollArea` around the same drawing. Positions inside it are worked out from where
    // the area put its cursor, so the blocks move with the scroll rather than being drawn at fixed places.
    let list = Rect::from_min_max(Pos2::new(area.min.x, pen), Pos2::new(area.max.x, room_for_comments));
    let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(list));
    let scrolled = egui::ScrollArea::vertical()
        .id_salt("agent-tasks-comments")
        .max_height(list.height().max(0.0))
        .show(&mut inside, |ui| {
            let painter = ui.painter().clone();
            let mut pen = ui.cursor().min.y;
            let top = pen;
            let mut resend: Option<String> = None;
            let mut edit: Option<i64> = None;
            let mut save = false;
            let mut cancel = false;
            let mut typed = false;
            // Which comment's view was changed, acted on once the loop has finished for the reason every other
            // change in it is: `board` is borrowed for the drawing.
            let mut view_change: Option<(i64, bool)> = None;
            let markdown = &mut board.markdown;
            for comment in comments.iter().rev() {
        text(
            &painter,
            Pos2::new(area.min.x, pen),
            &format!("{} · {}", comment.author.name(), clock::relative(&comment.created_at, &now)),
            look.font_size - 2.5,
            look.palette.text_faint,
        );
        // The two view buttons on the comment's own header row, right aligned, the same pair the description has.
        // Left of the `Edit` and `Send` buttons a person's own comment carries, which is why they stop short of
        // the right edge.
        if let Some(as_markdown) = super::raw_or_rendered(
            ui,
            look,
            Rect::from_min_size(
                Pos2::new(area.min.x, pen - look.font_size - 1.0),
                Vec2::new(area.width() - 112.0, 16.0),
            ),
            &format!("comment {}", comment.id),
            !raw_comments.contains(&comment.id),
        ) {
            view_change = Some((comment.id, !as_markdown));
        }
        pen += look.font_size + 1.0;
        let mine = comment.author == crate::services::agent_tasks::model::Author::Human;
        let being_edited = editing == Some(comment.id);
        let height = match being_edited {
            // A field the height of two comment lines, which is what an edit needs and what the section has room
            // for. Multiline, because a comment is prose and `Enter` in it has to make a line rather than save.
            true => {
                let at = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(area.width(), 48.0));
                painter.rect(
                    at,
                    CornerRadius::same(look.corner_radius as u8),
                    look.palette.field,
                    egui::Stroke::new(1.0, look.palette.control_border),
                    egui::StrokeKind::Inside,
                );
                let edit_id = ui.id().with("agent-tasks-comment-edit");
                let response = ui.put(
                    crate::components::controls::field_takes_the_whole_rectangle(ui, at, 6.0, edit_id),
                    egui::TextEdit::multiline(&mut edited)
                        .id(edit_id)
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(look.font_size - 1.0))
                        .text_color(look.palette.text),
                );
                if response.changed() {
                    typed = true;
                }
                48.0
            }
            // **Rendered by default.** `task-28`: a comment is read far more often than it is written, and an
            // agent's comments are markdown with headings, lists and code in them, so the source is the second
            // view rather than the first. `raw` below is the source, unchanged from what this always drew.
            false if !raw_comments.contains(&comment.id) => {
                let colors = crate::components::markdown_text::Colors {
                    text: look.palette.text_control,
                    strong: look.palette.text_strong,
                    code: look.palette.added,
                    link: look.palette.accent,
                    quiet: look.palette.text_dim,
                    rule: look.palette.divider,
                };
                let key = format!("comment-{}", comment.id);
                let made = markdown.rendered(
                    &key,
                    &comment.body,
                    look.renderer,
                    &look.font_family,
                    look.font_size - 1.0,
                    colors,
                    area.width(),
                    None,
                );
                // Clipped to sixty points, which is what the source view is clipped to: a very long comment is
                // read in the modal's own scrolling list rather than by one block growing without limit.
                let height = made.height().min(60.0);
                let block = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(area.width(), height));
                crate::components::markdown_text::show(ui, block, made, look.renderer, 0.0);
                height
            }
            false => {
                let galley = painter.layout(
                    comment.body.clone(),
                    egui::FontId::proportional(look.font_size - 1.0),
                    look.palette.text_control,
                    area.width(),
                );
                // Clipped to what was drawn rather than laid out for. A three thousand word comment used to paint
                // its whole galley while the layout moved on by sixty points, so it drew over everything under it.
                let height = galley.size().y.min(60.0);
                let block = Rect::from_min_size(Pos2::new(area.min.x, pen), Vec2::new(area.width(), height));
                painter.with_clip_rect(block).galley(block.min, galley, look.palette.text_control);
                height
            }
        };
        // A human's own comment can be sent to the agent on its own, which is the browser's `Send to terminal` on
        // each comment: a comment written before the agent was running still has to be able to reach it. And it
        // can be changed, which is the browser's `Edit`. Neither is drawn on an agent's comment: what an agent
        // said is a record, and the store refuses to change one whatever is pressed.
        if mine {
            let row = pen - look.font_size - 2.0;
            match being_edited {
                // `Save` and `Cancel` in place of the two, because while a comment is being edited those are the
                // only two things to do with it.
                true => {
                    let save_at = Rect::from_min_size(Pos2::new(area.max.x - 106.0, row), Vec2::new(50.0, 18.0));
                    let cancel_at = Rect::from_min_size(Pos2::new(area.max.x - 52.0, row), Vec2::new(52.0, 18.0));
                    if ui
                        .push_id(("agent-tasks-comment-save", comment.id), |ui| {
                            crate::components::controls::choice_button(ui, save_at, "Save", true)
                        })
                        .inner
                    {
                        save = true;
                    }
                    if ui
                        .push_id(("agent-tasks-comment-cancel", comment.id), |ui| {
                            crate::components::controls::choice_button(ui, cancel_at, "Cancel", false)
                        })
                        .inner
                    {
                        cancel = true;
                    }
                }
                false => {
                    let edit_at = Rect::from_min_size(Pos2::new(area.max.x - 144.0, row), Vec2::new(40.0, 18.0));
                    let send_at = Rect::from_min_size(Pos2::new(area.max.x - 100.0, row), Vec2::new(100.0, 18.0));
                    if ui
                        .push_id(("agent-tasks-comment-edit", comment.id), |ui| {
                            crate::components::controls::choice_button_named(
                                ui,
                                edit_at,
                                "Edit",
                                // Which comment, because a ticket has several and every one of these said
                                // only `Edit`: a screen reader met four controls with one name between them.
                                // The author and when it was written, which is what the heading above the comment
                                // says and what tells two comments by the same author apart. `push_id` makes the
                                // internal id unique and does nothing for the name a screen reader reads.
                                &format!(
                                    "Edit the comment by {} {}",
                                    comment.author.name(),
                                    clock::relative(&comment.created_at, &now)
                                ),
                                false,
                            )
                        })
                        .inner
                    {
                        edit = Some(comment.id);
                    }
                    if ui
                        .push_id(("agent-tasks-resend", comment.id), |ui| {
                            crate::components::controls::choice_button_named(
                                ui,
                                send_at,
                                "Send to terminal",
                                &format!(
                                    "Send the comment by {} {} to the terminal",
                                    comment.author.name(),
                                    clock::relative(&comment.created_at, &now)
                                ),
                                false,
                            )
                        })
                        .inner
                    {
                        resend = Some(comment.body.clone());
                    }
                }
            }
        }
                pen += height + 8.0;
            }
            // The room the comments really took, so the scroll area knows how far there is to scroll. Without it
            // an area whose content is painted rather than laid out believes it has nothing in it and never
            // scrolls.
            ui.allocate_space(Vec2::new(area.width(), (pen - top).max(0.0)));
            (resend, edit, save, cancel, typed, view_change)
        })
        .inner;
    let (resend, edit, save, cancel, typed, view_change) = scrolled;
    if let Some((id, raw)) = view_change {
        board.show_the_comment_raw(id, raw);
    }
    // Acted on after the drawing, because the comments are being read while they are drawn.
    if typed {
        board.detail_mut().comment_edit = edited;
    }
    if let Some(id) = edit {
        board.edit_the_comment(id);
    }
    if cancel {
        board.stop_editing_the_comment();
    }
    if save {
        match board.save_the_comment() {
            Ok(said) if !said.is_empty() => requests.push(Request::Message(said)),
            Ok(_) => {}
            Err(problem) => requests.push(Request::Message(problem)),
        }
    }
    if let Some(body) = resend {
        match board.send_a_comment(&body) {
            Ok(said) if !said.is_empty() => requests.push(Request::Message(said)),
            Ok(_) => {}
            Err(problem) => requests.push(Request::Message(problem)),
        }
    }
    let at = Rect::from_min_size(
        Pos2::new(area.min.x, area.max.y - box_height),
        Vec2::new(area.width(), look.row_height),
    );
    painter.rect(
        at,
        CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    let mut draft = board.detail().draft.clone();
    let draft_id = ui.id().with("agent-tasks-comment-draft");
    let response = ui.put(
        crate::components::controls::field_takes_the_whole_rectangle(ui, at, 8.0, draft_id),
        egui::TextEdit::singleline(&mut draft)
            .id(draft_id)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new("Add a comment").color(look.palette.text_faint))
            .font(egui::FontId::proportional(look.font_size - 0.5))
            .text_color(look.palette.text),
    );
    if response.changed() {
        board.detail_mut().draft = draft;
    }
    let buttons_y = at.max.y + 4.0;
    let post = Rect::from_min_size(Pos2::new(at.min.x, buttons_y), Vec2::new(90.0, 22.0));
    let send = Rect::from_min_size(Pos2::new(at.min.x + 96.0, buttons_y), Vec2::new(120.0, 22.0));
    let posting = crate::components::controls::choice_button(ui, post, "Post comment", false)
        || super::enter_was_used_and_pressed(ui, &response);
    let sending = crate::components::controls::choice_button(ui, send, "Send to terminal", false);
    if posting || sending {
        match board.post_the_comment(sending) {
            Ok(said) if !said.is_empty() => requests.push(Request::Message(said)),
            Ok(_) => {}
            Err(problem) => requests.push(Request::Message(problem)),
        }
    }
    requests
}
