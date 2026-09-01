//! One card on the board.
//!
//! The card the design image shows: the title, the epic's chip, the JIRA key, a priority chevron, the
//! ticket key, the todo counts, the comment count, a start button and the agent badge. Nothing here
//! decides anything; it paints one ticket and says what was pressed.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::{darken, lighten, text};
use crate::services::vello_canvas::{Fill, Lift};
use crate::services::agent_tasks::board;
use crate::services::agent_tasks::model::{Board, Priority, Task};
use crate::services::plugin_ui::Look;
use crate::theme::icon;

/// How tall a card is at the default font size, and the gap between two.
///
/// Read through [`height`] rather than directly, so that a window set to 48 point text gets cards that can hold
/// 48 point text. See `Look::scale`.
const HEIGHT_AT_DEFAULT: f32 = 100.0;
pub const GAP: f32 = 24.0;

/// How round a card's corners are, and how big the round buttons along its footer are.
///
/// Measured off `_agent_output/task-1765-vello-board/reference-board.png`: a card there is 300 by 101 with a
/// 14 point radius and 24 points between two, its play button is 30 across and its agent badge 28, and the
/// ring round an attached badge is 2 points wide at radius 17.5. Two sizes rather than one, because the
/// reference has two: the button you press is the larger of the pair.
pub const RADIUS: f32 = 14.0;
const PLAY: f32 = 30.0;
const BADGE: f32 = 28.0;
/// How far outside the badge the attached ring is drawn, and how thick it is.
const RING: f32 = 17.5;
const RING_WIDTH: f32 = 2.0;

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
    let radius = CornerRadius::same(RADIUS as u8);
    // A card under the pointer wears the pill every list in Quill draws for its chosen row, rather than a
    // colour of its own — and it is a wash over the same decoration rather than a different elevation, for
    // the reason written below.
    let ground = match response.hovered() || dragging {
        true => look.palette.selected_row,
        false => look.palette.board_card,
    };
    let painter = ui.painter().clone();
    // **A card stands off its lane rather than being a rectangle drawn on it.** With the decoration on, that
    // is a pair of soft shadows and the surface over them. With it off — no manifest key, `plugins.chrome`
    // switched off, or a `Look` built by a test — the flat form is drawn instead, which is what the board was
    // before `task-1765`.
    if look.chrome.is_recording() {
        // **The elevation does not change under the pointer, and that is a performance rule rather than a
        // taste one.** The decoration is one texture rasterised on the processor whenever the board's
        // drawing changes; a card that lifted on hover would re-rasterise the whole pane every time the
        // pointer crossed a card, which is the commonest thing anybody does on a board. So the depth is a
        // property of the card and the pointer's answer is a wash painted over it in `egui`, which costs
        // nothing — and it is the same pill every list in Quill draws for the row it is on.
        look.chrome.raised(area, RADIUS, Fill::Solid(look.palette.board_card), Lift::Small);
        if response.hovered() || dragging {
            painter.rect_filled(area, radius, ground.gamma_multiply(0.5));
        }
    } else {
        painter.rect(area, radius, ground, Stroke::new(1.0, look.palette.control_border), egui::StrokeKind::Inside);
    }
    // The epic's colour down the left edge. The one colour on the board that comes from the data.
    if let Some(colour) = task.epic_id.and_then(|id| board.epic(id)).and_then(|epic| crate::services::plugins::colour(&epic.color)) {
        painter.rect_filled(
            Rect::from_min_size(area.min, Vec2::new(EDGE, area.height())),
            CornerRadius { nw: radius.nw, sw: radius.sw, ne: 0, se: 0 },
            Color32::from_rgb(colour.r, colour.g, colour.b),
        );
    }
    // **Every measurement inside a card scales with the editor's font**, which the boxes have done since
    // `Look::scale` was written and the contents had not. At 32 point text the footer band was still 26
    // points from the bottom of a 200 point card, so the two round buttons hung out of the bottom of the
    // card and were clipped away by the lane — a card with no play button on it. Found at the width the
    // rail's own boundary test drives, which is why that test earns its place twice over.
    let scale = look.scale();
    let left = area.min.x + EDGE + 10.0 * scale;
    let right = area.max.x - 10.0 * scale;
    super::clipped_in(
        &painter,
        Pos2::new(left, area.min.y + 12.0 * scale),
        task.display_title(),
        egui::FontId::new(
            look.font_size - 1.0,
            egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into()),
        ),
        look.palette.text_strong,
        right - left,
        // Two lines, which is what a card 100 points tall has room for above its own footer.
        2,
    );
    // The priority mark on a line of its own under the title, which is where the design puts it, rather
    // than beside the title where a title that wrapped to two lines would run into it.
    priority(&painter, Pos2::new(left + 5.0 * scale, area.max.y - 34.0 * scale), task.priority, look);
    // The epic's name on the epic's colour, and the JIRA key beside it, which is what the reference capture puts
    // under the title. The coloured left edge alone said an epic existed without saying which.
    let mut chip = left + 16.0 * scale;
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
    let footer = area.max.y - 26.0 * scale;
    // The round buttons stop growing before everything else does: at three times the default font a 90
    // point play button would be most of the card, and a target is big enough once it is big enough.
    let buttons = scale.min(1.6);
    let badge = Vec2::splat(BADGE * buttons);
    let play = Vec2::splat(PLAY * buttons);
    let badge_at =
        Rect::from_min_size(Pos2::new(right - badge.x, footer - 4.0 * scale), badge);
    agent_badge(&painter, badge_at, task, look, live);
    // The start button only when starting would do something. **Absent rather than dimmed**, which is the rule
    // the `F` button and the three code navigation entries already follow: a card whose agent is already running
    // has nothing for Start to do, and drawing it would be drawing a control that reports a refusal.
    let mut controls = badge_at.min.x;
    if live.can_start {
        let start_at = Rect::from_min_size(
            Pos2::new(badge_at.min.x - play.x - 8.0 * scale, badge_at.center().y - play.y / 2.0),
            play,
        );
        if super::round_button(ui, look, start_at, &format!("Start {}", task.key), icon::run) {
            pressed.start = true;
        }
        controls = start_at.min.x;
    }
    // Where the counts have to stop, with a gap so they do not touch what is beside them.
    let stop = controls - 6.0 * scale;
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
        pen += 10.0 * scale;
    }
    // Both are drawn even at zero — which is what the reference capture shows, `0/0` and `0` — because a
    // count that vanished at zero moved the ones beside it every time a ticket gained its first todo.
    // **A mark and its number are one decision, laid out once.** They were two — a test of the mark's own
    // width and then `room_for` on the number — and a card narrow enough for the first and not the second
    // wore a tick on its own, which says nothing at all. One measurement cannot disagree with itself.
    let mut counted = |painter: &egui::Painter,
                       pen: &mut f32,
                       mark: fn(&egui::Painter, Pos2, Color32),
                       gap: f32,
                       said: &str,
                       tint: Color32| {
        let galley =
            painter.layout_no_wrap(said.to_owned(), egui::FontId::proportional(look.font_size - 2.0), tint);
        if *pen + gap + galley.size().x > stop {
            return;
        }
        mark(painter, Pos2::new(*pen + gap * 0.4, footer + 6.0 * scale), tint);
        painter.galley(Pos2::new(*pen + gap, footer), galley.clone(), tint);
        *pen += gap + galley.size().x + 12.0 * scale;
    };
    // Both counts, with the marks the reference draws beside them. The comment count was a bare number,
    // which read as a second half of the todo count rather than as a count of something else.
    counted(
        &painter,
        &mut pen,
        icon::tick,
        14.0 * scale,
        &format!("{}/{}", task.todo_done_count, task.todo_count),
        tint,
    );
    counted(
        &painter,
        &mut pen,
        icon::comment,
        16.0 * scale,
        &task.comment_count.to_string(),
        look.palette.text_dim,
    );
    if response.clicked() {
        pressed.open = true;
    }
    if response.drag_started() {
        pressed.drag = true;
    }
    pressed
}

/// The chevron that says how urgent a ticket is, when it is urgent at all.
///
/// A drawn mark rather than a word, because a card in a 300 point lane has no room for `High priority`
/// and a mark is what the reference draws. Two of them: a doubled chevron for high and a single one for
/// medium, both pointing up. Low draws nothing — see below.
fn priority(painter: &egui::Painter, at: Pos2, priority: Priority, look: &Look<'_>) {
    let scale = look.scale();
    // **Up for urgent, and nothing at all for low.**
    //
    // The direction was inverted: `High` and `Medium` drew a `v` and `Low` drew a `^`, so every card on the
    // board wore a downward chevron and the one ticket that mattered least wore the upward one. Nothing
    // points down any more.
    //
    // And low is now silent, which is the rule the rest of Quill keeps: a mark is drawn to say a thing is
    // *unusual*, and low is what most tickets are. A downward chevron on every ordinary card is a mark that
    // says nothing while taking up the place a mark that says something would go — and the reference draws
    // no low-priority mark either.
    let (tint, direction) = match priority {
        Priority::High => (look.palette.unsaved, 1.0),
        Priority::Medium => (look.palette.text_control, 1.0),
        Priority::Low => return,
    };
    let width = 4.0 * scale;
    let height = 3.0 * scale * direction;
    painter.line_segment(
        [Pos2::new(at.x - width, at.y + height), Pos2::new(at.x, at.y - height)],
        Stroke::new(1.4 * scale, tint),
    );
    painter.line_segment(
        [Pos2::new(at.x, at.y - height), Pos2::new(at.x + width, at.y + height)],
        Stroke::new(1.4 * scale, tint),
    );
    // High priority draws the mark twice, which is the double chevron the design image shows.
    if priority == Priority::High {
        painter.line_segment(
            [Pos2::new(at.x - width, at.y + height * 2.4), Pos2::new(at.x, at.y)],
            Stroke::new(1.4 * scale, tint),
        );
        painter.line_segment(
            [Pos2::new(at.x, at.y), Pos2::new(at.x + width, at.y + height * 2.4)],
            Stroke::new(1.4 * scale, tint),
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
    // The violet the picture shows, dimmed for a ticket nobody has: the badge says *who*, and the ring round
    // it says *now*. Two marks rather than one brightness, which is what made an unattached ticket and an
    // attached one nearly the same colour.
    let ground = match task.assignee {
        crate::services::agent_tasks::model::Assignee::Human => look.palette.control_border,
        _ => look.palette.agent,
    };
    let ground = match live.attached {
        true => ground,
        false => ground.gamma_multiply(0.72),
    };
    // The ring's own measurements, in proportion to the badge: 17.5 outside a 28 point badge, 2 points wide.
    let ring = area.width() * (RING / BADGE);
    let width = area.width() * (RING_WIDTH / BADGE);
    if look.chrome.is_recording() {
        let radius = area.width() / 2.0;
        look.chrome.disc(
            area.center(),
            radius,
            Fill::diagonal(area, lighten(ground, 0.10), darken(ground, 0.16)),
        );
        // The mint ring, and only while a terminal for this ticket is really running in this window. The gap
        // between the badge and the ring is the picture's: two points of dark, then two points of green.
        if live.attached {
            look.chrome.ring(area.center(), ring, width, look.palette.attached);
        }
    } else {
        painter.circle_filled(area.center(), area.width() / 2.0, ground);
        if live.attached {
            painter.circle_stroke(area.center(), ring, Stroke::new(width, look.palette.attached));
        }
    }
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
