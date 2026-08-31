//! One card on the board.
//!
//! The card the design image shows: the title, the epic's chip, the JIRA key, a priority chevron, the
//! ticket key, the todo counts, the comment count, a start button and the agent badge. Nothing here
//! decides anything; it paints one ticket and says what was pressed.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::{clipped, text};
use crate::services::agent_tasks::board;
use crate::services::agent_tasks::model::{Board, Priority, Task};
use crate::services::plugin_ui::Look;
use crate::theme::icon;

/// How tall a card is at the default font size, and the gap between two.
///
/// Read through [`height`] rather than directly, so that a window set to 48 point text gets cards that can hold
/// 48 point text. See `Look::scale`.
const HEIGHT_AT_DEFAULT: f32 = 84.0;
pub const GAP: f32 = 8.0;

/// How tall a card is in this window.
pub fn height(look: &Look<'_>) -> f32 {
    HEIGHT_AT_DEFAULT * look.scale()
}
/// How wide the coloured edge that names the epic is.
const EDGE: f32 = 3.0;

/// What a card reported this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pressed {
    /// The card itself was clicked, which opens the ticket.
    pub open: bool,
    /// The start button was pressed, which launches the agent and hands the ticket over.
    pub start: bool,
    /// A drag began on this card.
    pub drag: bool,
}

/// Draw one card and say what was pressed.
/// What the window knows about a ticket that the row does not.
///
/// Two things, and both decide whether a control is drawn at all: whether this ticket has a terminal running
/// in **this** window, and whether Start could do anything. Quill's rule is that a control which cannot
/// apply is absent, so a card with an agent running draws no Start button, and a card in a lane nothing can
/// be started from draws none either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Live {
    /// A terminal for this ticket is running in this window.
    pub attached: bool,
    /// Start would do something: the ticket is not already claimed and is not a person's.
    pub can_start: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    task: &Task,
    board: &Board,
    dragging: bool,
    live: Live,
) -> Pressed {
    let mut pressed = Pressed::default();
    let response = ui.interact(area, ui.id().with(("agent-tasks-card", task.id)), Sense::click_and_drag());
    // Named, because every control in Quill has a plain name and a test finds one by it. A card is the control
    // that opens a ticket, and it was findable only by its position.
    let name = format!("{} {}", task.key, task.display_title());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.clone())
    });
    let radius = CornerRadius::same(look.corner_radius as u8);
    // A card under the pointer is the pill every list in Quill draws for its chosen row, rather than a
    // colour of its own.
    let ground = match response.hovered() || dragging {
        true => look.palette.selected_row,
        false => look.palette.control,
    };
    let painter = ui.painter().clone();
    painter.rect(area, radius, ground, Stroke::new(1.0, look.palette.control_border), egui::StrokeKind::Inside);
    // The epic's colour down the left edge. The one colour on the board that comes from the data.
    if let Some(colour) = task.epic_id.and_then(|id| board.epic(id)).and_then(|epic| crate::services::plugins::colour(&epic.color)) {
        painter.rect_filled(
            Rect::from_min_size(area.min, Vec2::new(EDGE, area.height())),
            CornerRadius { nw: radius.nw, sw: radius.sw, ne: 0, se: 0 },
            Color32::from_rgb(colour.r, colour.g, colour.b),
        );
    }
    let left = area.min.x + EDGE + 10.0;
    let right = area.max.x - 10.0;
    clipped(
        &painter,
        Pos2::new(left, area.min.y + 8.0),
        task.display_title(),
        look.font_size,
        look.palette.text_strong,
        right - left,
        // Two lines, which is what a card 84 points tall has room for above its own footer.
        2,
    );
    // The priority mark on a line of its own under the title, which is where the design puts it, rather
    // than beside the title where a title that wrapped to two lines would run into it.
    priority(&painter, Pos2::new(left + 5.0, area.max.y - 34.0), task.priority, look);
    // The epic's name on the epic's colour, and the JIRA key beside it, which is what the reference capture puts
    // under the title. The coloured left edge alone said an epic existed without saying which.
    let mut chip = left + 16.0;
    if let Some(epic) = task.epic_id.and_then(|id| board.epic(id)) {
        let colour = crate::services::plugins::colour(&epic.color)
            .map(|colour| Color32::from_rgb(colour.r, colour.g, colour.b))
            .unwrap_or(look.palette.control_border);
        let galley = painter.layout_no_wrap(
            epic.name.clone(),
            egui::FontId::proportional(look.font_size - 3.0),
            look.palette.text_strong,
        );
        let at = Rect::from_min_size(
            Pos2::new(chip, area.max.y - 40.0),
            Vec2::new(galley.size().x + 10.0, 14.0),
        );
        if at.max.x < right {
            painter.rect_filled(at, CornerRadius::same(3), colour);
            painter.galley(
                Pos2::new(at.min.x + 5.0, at.center().y - galley.size().y / 2.0),
                galley,
                look.palette.text_strong,
            );
            chip = at.max.x + 6.0;
        }
    }
    if let Some(key) = &task.jira_key {
        if chip + 60.0 < right {
            // **In the dim text colour, not the accent one.** It was drawn in `palette.modified`, which is the
            // colour Quill uses for something you can act on, so it read as a link — and clicking it opened the
            // ticket modal, because the card's own click target covers the whole card and there is nothing here
            // that could open a browser. Quill has no way to open a URL: `services::launcher` opens a folder in
            // the file manager and a new Quill window, and that is all. The link lives on `Copy issue link` in the
            // ticket's JIRA panel, which hands the address to the clipboard. So this is a label saying which
            // issue the ticket is about, and it now looks like one.
            text(&painter, Pos2::new(chip, area.max.y - 40.0), key, look.font_size - 3.0, look.palette.text_dim);
        }
    }
    // The footer: the key, the counts, and the controls on the right.
    //
    // The controls are placed first and the counts are stopped short of them. A lane squeezed to `LANE_MIN`
    // leaves a card about 200 points wide, and the counts used to march right from the key without knowing the
    // badge was there, so the comment count and the agent badge were drawn on top of each other — two sets of
    // glyphs in one place, which is what the tab screenshot recorded in the Agent Done lane.
    let footer = area.max.y - 22.0;
    let badge = Vec2::splat(20.0);
    let badge_at = Rect::from_min_size(Pos2::new(right - badge.x, footer - 2.0), badge);
    agent_badge(&painter, badge_at, task, look, live);
    // The start button only when starting would do something. **Absent rather than dimmed**, which is the rule
    // the `F` button and the three code navigation entries already follow: a card whose agent is already running
    // has nothing for Start to do, and drawing it would be drawing a control that reports a refusal.
    let mut controls = badge_at.min.x;
    if live.can_start {
        let start_at = Rect::from_min_size(Pos2::new(right - badge.x - 26.0, footer - 2.0), badge);
        if crate::components::controls::icon_button(ui, start_at, &format!("Start {}", task.key), icon::run) {
            pressed.start = true;
        }
        controls = start_at.min.x;
    }
    // Where the counts have to stop, with a gap so they do not touch what is beside them.
    let stop = controls - 6.0;
    let mut pen = left;
    let tint = match board::todos_complete(task) {
        true => look.palette.added,
        false => look.palette.text_dim,
    };
    // Each piece is drawn only if the whole of it fits before the controls. The key first, because a card that
    // can show only one thing should show which ticket it is.
    let room_for = |painter: &egui::Painter, pen: &mut f32, said: &str, tint: Color32| {
        let galley =
            painter.layout_no_wrap(said.to_owned(), egui::FontId::proportional(look.font_size - 2.0), tint);
        if *pen + galley.size().x > stop {
            return false;
        }
        painter.galley(Pos2::new(*pen, footer), galley.clone(), tint);
        *pen += galley.size().x;
        true
    };
    if room_for(&painter, &mut pen, &task.key, look.palette.text_dim) {
        pen += 10.0;
    }
    // Both counts, always, including at zero — which is what the reference capture shows: `0/0` and `0`. A count
    // that vanished at zero moved the ones beside it every time a ticket gained its first todo.
    if pen + 14.0 < stop {
        icon::tick(&painter, Pos2::new(pen + 5.0, footer + 6.0), tint);
        pen += 14.0;
        if room_for(&painter, &mut pen, &format!("{}/{}", task.todo_done_count, task.todo_count), tint) {
            pen += 12.0;
        }
    }
    room_for(&painter, &mut pen, &task.comment_count.to_string(), look.palette.text_dim);
    if response.clicked() {
        pressed.open = true;
    }
    if response.drag_started() {
        pressed.drag = true;
    }
    pressed
}

/// The chevron that says how urgent a ticket is.
///
/// Three drawn marks rather than three words, because a card in a 300 point lane has no room for
/// `High priority` and the mark is what the design image shows.
fn priority(painter: &egui::Painter, at: Pos2, priority: Priority, look: &Look<'_>) {
    let (tint, direction) = match priority {
        Priority::High => (look.palette.unsaved, -1.0),
        Priority::Medium => (look.palette.text_control, -1.0),
        Priority::Low => (look.palette.text_dim, 1.0),
    };
    let width = 4.0;
    let height = 3.0 * direction;
    painter.line_segment(
        [Pos2::new(at.x - width, at.y + height), Pos2::new(at.x, at.y - height)],
        Stroke::new(1.4, tint),
    );
    painter.line_segment(
        [Pos2::new(at.x, at.y - height), Pos2::new(at.x + width, at.y + height)],
        Stroke::new(1.4, tint),
    );
    // High priority draws the mark twice, which is the double chevron the design image shows.
    if priority == Priority::High {
        painter.line_segment(
            [Pos2::new(at.x - width, at.y + height * 2.4), Pos2::new(at.x, at.y)],
            Stroke::new(1.4, tint),
        );
        painter.line_segment(
            [Pos2::new(at.x, at.y), Pos2::new(at.x + width, at.y + height * 2.4)],
            Stroke::new(1.4, tint),
        );
    }
}

/// The round mark saying who has the ticket, brighter while a terminal is attached.
///
/// The one piece of state on the board that changes without anybody pressing anything, which is why it is a
/// brightness rather than a word: it has to be readable at a glance across four lanes.
///
/// **Attached means a terminal running now**, not a session id on the row. The row keeps its session id for
/// ever, because that is what a resume names, so reading brightness off the row made every ticket that had
/// ever been worked look as though its agent were still there.
fn agent_badge(painter: &egui::Painter, area: Rect, task: &Task, look: &Look<'_>, live: Live) {
    let ground = match live.attached {
        true => look.palette.accent,
        false => look.palette.control_border,
    };
    painter.circle_filled(area.center(), area.width() / 2.0, ground);
    let initials = match task.assignee {
        crate::services::agent_tasks::model::Assignee::Claude => "C",
        crate::services::agent_tasks::model::Assignee::Codex => "X",
        crate::services::agent_tasks::model::Assignee::Human => "JM",
    };
    let galley = painter.layout_no_wrap(
        initials.to_owned(),
        egui::FontId::proportional(look.font_size - 3.0),
        look.palette.text_strong,
    );
    painter.galley(area.center() - galley.size() / 2.0, galley, look.palette.text_strong);
}

/// `1 comment`, `2 comments`. One function, because three places on the board count things and a board
/// that says `1 comments` reads as a board nobody looked at.
pub(crate) fn plural(count: i64, what: &str) -> String {
    match count {
        1 => format!("1 {what}"),
        other => format!("{other} {what}s"),
    }
}
