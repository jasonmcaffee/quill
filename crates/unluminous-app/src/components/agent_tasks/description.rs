//! A ticket's description, drawn by Unluminous's own editor.
//!
//! The description is markdown and Unluminous reads and draws markdown, so it is a `unluminous_core::Document` and
//! `components::editor_view` draws it. That gives it the real editor: the same font, the same syntax colouring
//! inside a code fence, the same undo, the same caret, the same selection. It is the largest single saving in
//! the plugin and it is the reason a task board inside a text editor is worth building at all.
//!
//! ## The document is the draft and the row is what was saved
//!
//! The document holds what somebody is typing. Every edit writes the whole text onto the row, which is what
//! "saves as you type" means and what the browser board does with a six hundred millisecond debounce. There is
//! no debounce here because there is no network: writing a column of a local SQLite row is a hundred
//! microseconds, measured by `examples/board_cost.rs`.

use egui::Rect;

use crate::services::agent_tasks::AgentTasks;
use crate::services::plugin_ui::{Look, Request};

/// Draw the description of the ticket that is open, and save what was typed.
///
/// Two views since `task-28`: the source, which is what somebody types into, and the same text rendered as
/// markdown, which is what somebody reads. Which one is showing is `Detail::description_rendered`, and the two
/// buttons that set it are drawn by the caller over the label, because that is where a label's own controls go.
pub fn show(board: &mut AgentTasks, ui: &mut egui::Ui, area: Rect, look: &Look<'_>) -> Vec<Request> {
    match board.detail().description_rendered {
        true => rendered(board, ui, area, look),
        false => raw(board, ui, area, look),
    }
}

/// The description as markdown, which can be read and not typed into.
///
/// `components::markdown_text` is what renders and paints it, which is `unluminous_core::markdown` and the editor's
/// own painter — so a fenced code block sits on a panel and a table has rules, exactly as in a `.md` file's
/// preview. What it does not have is pictures and Mermaid diagrams, for the reason that module records.
fn rendered(board: &mut AgentTasks, ui: &mut egui::Ui, area: Rect, look: &Look<'_>) -> Vec<Request> {
    use crate::components::markdown_text;
    ui.painter().rect(
        area,
        egui::CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    let inside = area.shrink(8.0);
    let source = board.description_text();
    // A description with nothing in it says so, rather than drawing an empty panel that looks like a fault.
    if source.trim().is_empty() {
        super::text(
            ui.painter(),
            inside.min,
            "Nothing written yet.",
            look.font_size - 0.5,
            look.palette.text_faint,
        );
        return Vec::new();
    }
    let colors = markdown_text::Colors {
        text: look.palette.text,
        strong: look.palette.text_strong,
        code: look.palette.added,
        link: look.palette.accent,
        quiet: look.palette.text_dim,
        rule: look.palette.divider,
    };
    let family = look.font_family.clone();
    let size = look.font_size;
    let renderer = look.renderer;
    // Split so the cache is borrowed on its own: `board.markdown` and `board.detail()` are two fields of one
    // struct and the borrow checker wants them named separately.
    let scroll = board.description_scroll;
    let made = board.markdown.rendered(
        "description",
        &source,
        renderer,
        &family,
        size,
        colors,
        inside.width(),
        None,
    );
    let height = made.height();
    markdown_text::show(ui, inside, made, renderer, scroll);
    // Scrolled with the wheel, clamped to what there is. A `ScrollArea` cannot be used here because the painting
    // is at absolute positions, which is the arrangement everything in this plugin already has.
    let over = ui.rect_contains_pointer(area);
    if over {
        let wheel = ui.ctx().input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 {
            board.description_scroll = (scroll - wheel).clamp(0.0, (height - inside.height()).max(0.0));
        }
    }
    Vec::new()
}

/// The description as its source, open for writing.
fn raw(board: &mut AgentTasks, ui: &mut egui::Ui, area: Rect, look: &Look<'_>) -> Vec<Request> {
    let mut requests = Vec::new();
    let painter = ui.painter().clone();
    // The panel ground the code blocks in a Markdown preview already sit on, so the description reads as a
    // field rather than as part of the modal.
    painter.rect(
        area,
        egui::CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    let inside = area.shrink(8.0);
    // **One copy of the description a frame, not three.** `description_text` copies it out of the ticket for the
    // field to edit, which a `TextEdit` needs; a second copy was made only to compare against afterwards, and the
    // ticket still holds the original to compare with. On a long description that second copy was the largest
    // thing this frame did, repeated sixty times a second while somebody typed.
    let mut text = board.description_text();
    let description_id = ui.id().with("agent-tasks-description");
    // The whole of the drawn box takes a click, not only the rectangle the text is laid out in — the
    // eight points of margin round it are part of the control. See `controls::claim_the_field`.
    crate::components::controls::claim_the_field(ui, area, description_id);
    let response = ui.put(
        inside,
        egui::TextEdit::multiline(&mut text)
            .id(description_id)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new("What needs doing, in markdown.").color(look.palette.text_faint))
            .desired_width(inside.width())
            .desired_rows(((inside.height() / (look.font_size * 1.4)) as usize).max(3))
            .font(egui::FontId::proportional(look.font_size - 0.5))
            .text_color(look.palette.text),
    );
    if response.changed() && board.detail().task.as_ref().is_some_and(|task| task.description != text) {
        if let Err(problem) = board.save_the_description(&text) {
            requests.push(Request::Message(problem));
        }
    }
    requests
}
