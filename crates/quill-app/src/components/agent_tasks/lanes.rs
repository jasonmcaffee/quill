//! The four lanes, and the four listings that are not lanes.
//!
//! Which lane a card is in and where a drag lands are `services::agent_tasks::board`; this paints them.

use egui::{CornerRadius, Pos2, Rect, Vec2};

use super::{card, text};
use crate::services::agent_tasks::model::Status;
use crate::services::agent_tasks::{board as arithmetic, AgentTasks};
use crate::services::plugin_ui::{Look, Request};

/// How wide a lane is when there is room, how narrow it is squeezed to before the board starts
/// scrolling instead, and the gap between two.
///
/// 300 points is what the design image shows. A pane can be 420 points wide down the side of the window,
/// which holds one lane and a sliver, so below the point where four lanes fit they are narrowed as far as
/// [`LANE_MIN`] and the board scrolls sideways after that. Narrowing further would leave a lane too thin
/// for a card's title, which is worse than scrolling.
const LANE: f32 = 328.0;
const LANE_MIN: f32 = 240.0;
const GAP: f32 = 22.0;
/// How round a lane's corners are, and how far its cards are inset from its edges.
///
/// Measured off `_agent_output/task-1765-vello-board/reference-board.png`: a lane there is 328 wide with a
/// 14 point inset either side, so its cards are 300, and its corner radius is 18 — which is the
/// stylesheet's `--r-lg`.
const LANE_RADIUS: f32 = 18.0;
const LANE_INSET: f32 = 14.0;
/// How tall the strip at the top of a lane holding its name and its count is.
/// How tall a lane's heading is at the default font size, read through `lane_header` so a window set to large
/// text gets a heading that can hold it. See `Look::scale`.
const LANE_HEADER_AT_DEFAULT: f32 = 57.0;
/// How much of a lane's foot is kept clear of cards, which is what `+ Add task` sits in.
const FOOT_AT_DEFAULT: f32 = 56.0;
/// How much of the New lane is kept clear under its heading, which is what the agent chooser and its play
/// button sit in. Reserved whether or not New holds a card, so the cards under it do not jump when the last
/// one is dragged out of the lane.
const QUICK_LAUNCH_AT_DEFAULT: f32 = 42.0;

/// How far below a lane's heading its cards start.
///
/// Only the New lane reserves the quick launch band, so this is a function of the lane rather than a
/// constant. Everything that positions a card reads it, including the line drawn where a dragged card would
/// land: an earlier version added the band to the drawing and not to that line, which put the line half a
/// card away from where the card actually went.
fn under_the_heading(status: Status, look: &Look<'_>) -> f32 {
    match status {
        Status::New => (LANE_HEADER_AT_DEFAULT + QUICK_LAUNCH_AT_DEFAULT) * look.scale(),
        _ => LANE_HEADER_AT_DEFAULT * look.scale(),
    }
}

/// How tall a lane's heading is in this window.
fn lane_header(look: &Look<'_>) -> f32 {
    LANE_HEADER_AT_DEFAULT * look.scale()
}

/// How much of a lane's foot is kept clear of cards in this window.
///
/// Only the New lane has anything down there — `+ Add task` — so every other lane's foot is the padding that
/// stops the last card touching the lane's own curve. It used to be the full 40 points for all four, which
/// with lanes that hug their contents left a band of empty lane under the last card of every one of them.
fn foot(status: Status, look: &Look<'_>) -> f32 {
    match status {
        Status::New => FOOT_AT_DEFAULT * look.scale(),
        _ => LANE_INSET * look.scale(),
    }
}
const PAD: f32 = 8.0;
/// How tall the well an empty lane draws is, which is also how much of an empty lane a card can be dropped on.
const EMPTY_WELL: f32 = 84.0;

/// How tall a run of cards is: one gap **between** each pair and none after the last.
///
/// One function rather than the arithmetic written out three times, because a lane is now as tall as its
/// contents and the three places have to agree exactly — a trailing gap counted in one of them and not in
/// the others put a scrollbar down a lane with nothing to scroll.
fn cards_tall(held: usize, look: &Look<'_>) -> f32 {
    match held {
        0 => 0.0,
        held => held as f32 * (card::height(look) + card::GAP) - card::GAP,
    }
}

/// What the window knows about a ticket that its row does not: whether its agent is running here, and
/// whether Start would do anything.
fn live_for(
    board: &AgentTasks,
    task: &crate::services::agent_tasks::model::Task,
) -> card::Live {
    let attached = board.terminal_for(task.id).is_some_and(|terminal| terminal.session.is_running());
    card::Live {
        attached,
        // Start claims an unclaimed ticket. One that already has a session is resumed instead, and one
        // assigned to a person is handed to the configured agent — which is still a start, so it counts.
        //
        // **And a Codex ticket whose process has gone can be started again.** Codex names its own sessions, so
        // the id on the ticket is only Quill's marker that a worker was here and there is nothing to resume. The
        // card showed no Start and the modal offered `Resume session`, which refused and told the person to press
        // Start — a button that was not drawn anywhere. The only thing that can be done to such a ticket is start
        // it afresh, so that is what is offered.
        can_start: task.session_id.is_none()
            || (!attached
                && !crate::services::agent_tasks::agent::can_resume(task.assignee)
                && task.assignee.is_an_agent()),
    }
}

/// The board: four lanes across, scrolling sideways when the pane is narrower than they are.
pub fn show(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Request> {
    let mut requests = Vec::new();
    // **Clicking the board gives it the keyboard**, which is what makes the arrow keys reach it at all. Added
    // before anything else is drawn, because egui gives a press to the last widget that overlaps it: a background
    // added first is under every card and every button, so it answers only for a press nothing else wanted.
    let background = ui.interact(area, ui.id().with("agent-tasks-board-background"), egui::Sense::click());
    // Named, because every control in Quill has a plain name and a test finds one by it. This is the one that
    // gives the board the keyboard.
    background.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), "Board background")
    });
    if background.clicked() {
        requests.push(Request::TakeTheKeyboard(true));
        // And away from a ticket's terminal, if it had them: the keys now move the ring on the board.
        board.focus_the_terminal(false);
    }
    // How wide a lane is here, and how far the board has been scrolled sideways. Four lanes at their full
    // width need 1236 points; anything narrower squeezes them to `LANE_MIN` and then scrolls.
    let room = area.width() - PAD * 2.0;
    let lanes = Status::ALL.len() as f32;
    let lane_width =
        ((room - GAP * (lanes - 1.0)) / lanes).clamp(LANE_MIN, LANE);
    let content = lane_width * lanes + GAP * (lanes - 1.0);
    let scroll = board.scroll_the_lanes(ui, area, content - room);
    let mut to_open = None;
    let mut to_start = None;
    let mut add_a_task = false;
    let mut next_agent = false;
    let mut start_the_next: Option<String> = None;
    let mut dragged = None;
    let mut dropped: Option<(Status, i64)> = None;
    let pointer = ui.ctx().pointer_interact_pos();
    let released = ui.ctx().input(|input| input.pointer.any_released());
    let query = board.query().to_owned();
    let dragging = board.dragging;
    let mut lane_lefts = Vec::new();

    // Laid out in three passes, and the order is a borrow rather than a preference: the geometry needs
    // nothing, the scroll offsets change the provider, and the drawing reads it. Doing them in one pass is
    // what made an earlier version clone the whole board every frame — every ticket's description copied to
    // satisfy the borrow checker, which on a board of any size is the largest thing the frame does.
    //
    // First: where each lane is, and how many cards it holds.
    let room_for_cards =
        |lane: Rect, status: Status| lane.height() - under_the_heading(status, look) - foot(status, look);
    let mut geometry: Vec<(Status, Rect, usize)> = Vec::new();
    for (index, status) in Status::ALL.into_iter().enumerate() {
        let left = area.min.x + PAD + index as f32 * (lane_width + GAP) - scroll;
        if left > area.max.x || left + lane_width < area.min.x {
            continue;
        }
        // **The lane keeps its full width and is clipped by the pane's edge**, rather than being shrunk to what
        // is visible. Clamping the rectangle to the viewport meant that a lane half off the right edge was a
        // narrower lane: its cards were re-laid out at the reduced width, so scrolling the board sideways made
        // the cards in the edge lane shrink and their text reflow as they went, instead of sliding behind the
        // edge the way a column of cards should. The clip rectangle each lane's cards are drawn into is what
        // stops the drawing escaping the pane.
        let held = board
            .board()
            .lane(status)
            .map(|lane| lane.tasks.iter().filter(|task| arithmetic::matches(task, &query)).count())
            .unwrap_or(0);
        // **A lane is as tall as what is in it**, which is what the picture shows: an empty lane is a short
        // box with a well in it and a full one runs to the bottom of the pane. A column of fixed height with
        // three cards in it and six hundred points of nothing under them is the thing that made the board
        // read as four empty troughs.
        //
        // Never shorter than its heading plus one card's worth of well, because that empty space is what a
        // card is dropped onto: a lane that shrank to its heading would be a lane nothing could be moved to.
        let cards_tall = match held {
            0 => EMPTY_WELL,
            held => cards_tall(held, look),
        };
        let wanted = under_the_heading(status, look) + cards_tall + foot(status, look);
        let available = area.height() - PAD * 2.0;
        let lane_area = Rect::from_min_size(
            Pos2::new(left, area.min.y + PAD),
            Vec2::new(lane_width, wanted.min(available).max(0.0)),
        );
        geometry.push((status, lane_area, held));
    }
    // Second: how far each of them is scrolled, which is the only thing here that changes anything.
    let downs: Vec<f32> = geometry
        .iter()
        .map(|(status, lane_area, held)| {
            let content = cards_tall(*held, look);
            let most = (content - room_for_cards(*lane_area, *status)).max(0.0);
            // The visible part, because a lane keeps its full width even when it runs off the edge and a wheel
            // over the pane next door is not a wheel over this lane.
            board.scroll_a_lane(ui, *status, lane_area.intersect(area), most)
        })
        .collect();
    // The keyboard, between the geometry and the drawing, because moving the ring needs the counts each lane
    // holds **after** the search has narrowed it and the drawing needs to know where the ring ended up.
    //
    // Only when the window says this plugin has the keys, and only when nothing is typing: `Look` carries the
    // first, and a text box or a modal holding the keyboard is what the other two check. Without all three, an
    // arrow key moving the caret in the editor would also move the ring on a board nobody was looking at.
    // All four lanes rather than the ones on screen, because the ring has to be able to cross to a lane the board
    // has scrolled past — and then the board is scrolled so that lane is showing.
    let counts = board.lane_counts();
    let mut open_the_chosen = None;
    if look.has_the_keyboard
        && !crate::app::text_box_has_the_keyboard(ui.ctx())
        && !crate::app::a_modal_has_the_keyboard(ui.ctx())
    {
        // **Consumed, not just read.** A key the board acts on has to be taken out of the frame's input, or
        // whatever is drawn after the board sees the same press: the Enter that opens a ticket was still there when
        // the modal drew, so the modal took it as its own confirm and shut again in the frame it opened.
        let (across, down, enter) = ui.ctx().input_mut(|input| {
            let none = egui::Modifiers::NONE;
            (
                i64::from(input.consume_key(none, egui::Key::ArrowRight))
                    - i64::from(input.consume_key(none, egui::Key::ArrowLeft)),
                i64::from(input.consume_key(none, egui::Key::ArrowDown))
                    - i64::from(input.consume_key(none, egui::Key::ArrowUp)),
                input.consume_key(none, egui::Key::Enter),
            )
        });
        if across != 0 || down != 0 {
            board.move_the_choice(&counts, across, down);
            if let Some((lane, _)) = board.chosen {
                board.show_the_lane(lane, lane_width, GAP, room, content - room);
            }
        }
        if enter {
            open_the_chosen = board.the_chosen_ticket(&counts);
        }
    }
    let ring = board.chosen;

    // Third: the drawing, which only reads.
    let snapshot = board.board();
    for (index, (status, lane_area, _)) in geometry.iter().copied().enumerate() {
        lane_lefts.push((status, lane_area.min.x));
        // A lane stands off the page rather than being a rectangle painted on it, and its cards are cut to
        // its own curve — which is the one thing on the board `egui` cannot do at all, since its clip
        // rectangle is square. The clip is closed after this lane's cards, below.
        if look.chrome.is_recording() {
            look.chrome.raised(
                lane_area,
                LANE_RADIUS,
                crate::services::vello_canvas::Fill::Solid(look.ground(look.palette.board_lane)),
                crate::services::vello_canvas::Lift::Medium,
            );
            look.chrome.clip(lane_area, LANE_RADIUS);
        } else {
            ui.painter().rect_filled(
                lane_area,
                CornerRadius::same(LANE_RADIUS as u8),
                look.ground(look.palette.board_lane),
            );
        }
        let cards: Vec<&crate::services::agent_tasks::model::Task> = snapshot
            .lane(status)
            .map(|lane| lane.tasks.iter().filter(|task| arithmetic::matches(task, &query)).collect())
            .unwrap_or_default();
        header(ui, look, lane_area, status, cards.len());
        // **A lane scrolls rather than stopping.** It used to draw as many cards as fit and then a `3 more`
        // that nothing could reach, so a card past the fold was a card nobody could open, tick or drag.
        let room = room_for_cards(lane_area, status);
        let content = cards_tall(cards.len(), look);
        let down = downs.get(index).copied().unwrap_or(0.0);
        let mut tops = Vec::new();
        let cards_area = Rect::from_min_max(
            Pos2::new(lane_area.min.x, lane_area.min.y + under_the_heading(status, look)),
            Pos2::new(lane_area.max.x, lane_area.max.y - foot(status, look)),
        );
        let mut lane_ui = ui.new_child(egui::UiBuilder::new().max_rect(cards_area));
        // Intersected with the board's own area, not replaced by the lane's. A lane now keeps its full width even
        // when half of it is off the edge, so its cards' rectangle reaches past the pane and the clip has to be
        // the part of it that is really on screen.
        lane_ui.set_clip_rect(cards_area.intersect(area));
        for (row, task) in cards.iter().enumerate() {
            let top = cards_area.min.y + row as f32 * (card::height(look) + card::GAP) - down;
            tops.push(top);
            if top + card::height(look) < cards_area.min.y || top > cards_area.max.y {
                continue;
            }
            let at = Rect::from_min_size(
                Pos2::new(lane_area.min.x + LANE_INSET, top),
                Vec2::new(lane_area.width() - LANE_INSET * 2.0, card::height(look)),
            );
            let pressed = card::show(
                &mut lane_ui,
                look,
                at,
                task,
                snapshot,
                dragging == Some(task.id),
                live_for(board, task),
            );
            // The ring, so the keyboard's place on the board can be seen. Drawn after the card, because the card
            // paints its own ground and border and a ring drawn first would be under them.
            if ring == Some((status, row)) {
                lane_ui.painter().rect_stroke(
                    at.expand(2.0),
                    CornerRadius::same(look.corner_radius as u8 + 1),
                    egui::Stroke::new(1.5, look.palette.accent),
                    egui::StrokeKind::Outside,
                );
            }
            if pressed.open {
                to_open = Some(task.id);
            }
            if pressed.start {
                to_start = Some(task.key.clone());
            }
            if pressed.drag {
                dragged = Some(task.id);
            }
        }
        // How much of the lane is out of sight, as the same thin bar the board's own sideways scroll draws.
        if content > room {
            let track = Rect::from_min_max(
                Pos2::new(lane_area.max.x - 4.0, cards_area.min.y),
                Pos2::new(lane_area.max.x - 2.0, cards_area.max.y),
            );
            let share = (room / content).clamp(0.08, 1.0);
            let at = down / (content - room);
            ui.painter().rect_filled(track, 0, look.palette.divider);
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(track.min.x, track.min.y + (track.height() - track.height() * share) * at),
                    Vec2::new(track.width(), track.height() * share),
                ),
                0,
                look.palette.text_faint,
            );
        }
        // Where a card being dragged would land in this lane, drawn as the accent line every drop target
        // in Quill is drawn as.
        if let (Some(_), Some(pointer)) = (dragging, pointer) {
            if lane_area.contains(pointer) {
                // The index among the cards that are **drawn**, turned into the index among all of them: with a
                // search narrowing the lane, dropping between the second and third visible card wrote `2` into a
                // lane whose second and third rows were different tickets, quietly reordering hidden ones.
                let among_visible = arithmetic::position_at(&tops, card::height(look), pointer.y);
                let position = arithmetic::among_all(
                    snapshot.lane(status).map(|lane| lane.tasks.as_slice()).unwrap_or_default(),
                    &query,
                    among_visible,
                );
                // **Drawn where the card will appear, which is the visible index and this lane's scroll.** The
                // line used to be placed from `position`, the index among all the cards including the ones a
                // search had hidden, and it ignored how far the lane was scrolled down — so in a long lane the
                // line was drawn a card's height away from the pointer for every row scrolled past, and in a
                // filtered lane it was drawn against a row nobody could see. Releasing still moved the card to
                // the right place; the line was simply pointing somewhere else.
                let y = cards_area.min.y
                    + among_visible as f32 * (card::height(look) + card::GAP)
                    - down
                    - card::GAP / 2.0;
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        Pos2::new(lane_area.min.x + LANE_INSET, y),
                        Vec2::new(lane_area.width() - LANE_INSET * 2.0, 2.0),
                    ),
                    0,
                    look.palette.accent,
                );
                if released {
                    dropped = Some((status, position));
                }
            }
        }
        // The agent chooser and its play button under the New lane's heading, which is where the reference
        // capture puts them: a quick launch that starts the **next** ticket in New with a chosen agent
        // without opening it.
        if status == Status::New {
            let tall = 34.0 * look.scale();
            let strip = Rect::from_min_size(
                Pos2::new(lane_area.min.x + LANE_INSET, lane_area.min.y + lane_header(look) + 4.0),
                Vec2::new(lane_area.width() - LANE_INSET * 2.0, tall),
            );
            let play = Rect::from_min_size(Pos2::new(strip.max.x - tall, strip.min.y), Vec2::splat(tall));
            let chooser = Rect::from_min_max(strip.min, Pos2::new(play.min.x - 8.0, strip.max.y));
            // The chooser is a field, so it is pressed into the lane rather than raised off it: the picture
            // draws it as a well with the agent's name in it, which is what `sunken` is.
            if look.chrome.is_recording() {
                look.chrome.sunken(
                    chooser,
                    chooser.height() / 2.0,
                    look.ground(look.palette.board_well),
                    crate::services::vello_canvas::Lift::Small,
                );
            }
            let chosen = board.configuration().agent;
            // One button that cycles rather than a dropdown, because egui keeps one popup open at a time and
            // there are two agents to choose between. Pressing it names the other one.
            if super::chooser_button(
                ui,
                look,
                chooser,
                chosen.name(),
                &format!("Agent for a new ticket: {}", chosen.name()),
            ) {
                next_agent = true;
            }
            // Nothing to start when New is empty, so the button is not drawn — but its room is still
            // reserved, so the cards do not move when the lane empties.
            if !cards.is_empty()
                && super::round_button(
                    ui,
                    look,
                    play,
                    &format!("Start the next ticket with {}", chosen.name()),
                    crate::theme::icon::run,
                )
            {
                start_the_next = cards.first().map(|task| task.key.clone());
            }
        }
        // `+ Add task` at the foot of the New lane, which is where the design image puts it.
        if status == Status::New {
            let at = Rect::from_min_size(
                Pos2::new(lane_area.min.x + LANE_INSET, lane_area.max.y - 42.0 * look.scale()),
                Vec2::new(lane_area.width() - LANE_INSET * 2.0, 42.0 * look.scale()),
            );
            // A well rather than a button, which is what the picture shows: the row where a card would go if
            // there were one, pressed into the lane.
            if look.chrome.is_recording() {
                look.chrome.sunken(
                    at,
                    card::RADIUS,
                    look.ground(look.palette.board_well),
                    crate::services::vello_canvas::Lift::Small,
                );
            }
            // With the decoration off, `choice_button_over` draws its own frame — see the `ground` argument
            // below — so there is nothing to add here.
            // Acted on after the loop, because the board is being read while it is drawn and creating a
            // ticket changes it. That is the rule every component here follows: report, then act.
            if crate::components::controls::choice_button_over(
                ui,
                at,
                "+ Add task",
                "+ Add task",
                false,
                !look.chrome.is_recording(),
            ) {
                add_a_task = true;
            }
        }
        // A lane with nothing in it says so, in a well the size of the space a card would take — which is
        // what the picture shows and is better than an empty box, because an empty box reads as a board that
        // failed to draw rather than as a lane nobody has filled.
        // Every lane, New included. New has `+ Add task` at its foot and could have been left with the
        // bare space above it, but a lane that is empty in one place and merely blank in another is two
        // answers to one question — and the space is the drop target, so saying what it is for is better
        // than leaving it to be guessed at.
        if cards.is_empty() {
            let empty = Rect::from_min_max(
                Pos2::new(cards_area.min.x + LANE_INSET, cards_area.min.y),
                Pos2::new(cards_area.max.x - LANE_INSET, (cards_area.min.y + EMPTY_WELL).min(cards_area.max.y)),
            );
            if empty.height() > 24.0 {
                match look.chrome.is_recording() {
                    true => look.chrome.sunken(
                        empty,
                        card::RADIUS,
                        look.ground(look.palette.board_well),
                        crate::services::vello_canvas::Lift::Medium,
                    ),
                    // The flat form, which is what a board with the decoration switched off draws. Every
                    // well on the board has one: a well drawn as nothing at all is a hole in the lane.
                    false => {
                        ui.painter().rect(
                            empty,
                            CornerRadius::same(card::RADIUS as u8),
                            look.ground(look.palette.board_well),
                            egui::Stroke::new(1.0, look.palette.divider),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                let said = ui.painter().layout_no_wrap(
                    "Nothing here".to_owned(),
                    egui::FontId::proportional(look.font_size - 1.0),
                    look.palette.text_faint,
                );
                ui.painter().galley(
                    empty.center() - said.size() / 2.0,
                    said,
                    look.palette.text_faint,
                );
            }
        }
        // The lane's own curve stops cutting here, which is the matching `unclip` for the `clip` above.
        if look.chrome.is_recording() {
            look.chrome.unclip();
        }
    }
    if next_agent {
        if let Err(problem) = board.use_the_next_agent() {
            requests.push(Request::Message(problem));
        }
    }
    if let Some(key) = start_the_next {
        match board.command_now("start", &[key]) {
            Ok(answer) => requests.push(Request::Message(answer.message)),
            Err(problem) => requests.push(Request::Message(problem)),
        }
        requests.push(Request::Repaint);
    }
    if add_a_task {
        match board.command_now("new-task", &[]) {
            // The new ticket's detail opens, which is what makes the editor able to name it: the row exists
            // from the moment it is created, so typing into the title saves as it is typed.
            Ok(_) => {}
            Err(problem) => requests.push(Request::Message(problem)),
        }
    }
    if let Some(id) = dragged {
        board.dragging = Some(id);
    }
    // How far there is left to scroll, drawn as a thin bar along the bottom when there is any, which is
    // the only thing that says the lanes go on past the edge.
    if content > room {
        // Five points, not two. It is the only thing on the board that says the lanes go on past the edge —
        // a fourth lane cut off by the pane with a two-point hairline under it reads as clipped rather than
        // as reachable — and it is the same bar `components::scrollbar` draws down the editing area.
        let track = Rect::from_min_max(
            Pos2::new(area.min.x + PAD, area.max.y - 7.0),
            Pos2::new(area.max.x - PAD, area.max.y - 2.0),
        );
        let share = (room / content).clamp(0.1, 1.0);
        let at = match content > room {
            true => scroll / (content - room),
            false => 0.0,
        };
        let thumb = Rect::from_min_size(
            Pos2::new(track.min.x + (track.width() - track.width() * share) * at, track.min.y),
            Vec2::new(track.width() * share, track.height()),
        );
        ui.painter().rect_filled(track, CornerRadius::same(2), look.palette.divider);
        ui.painter().rect_filled(thumb, CornerRadius::same(2), look.palette.text_dim);
    }
    if let Some((status, position)) = dropped {
        let moved = board.dragging.take();
        if let Some(id) = moved {
            if let Err(problem) = board.move_card(id, status, position) {
                requests.push(Request::Message(problem));
            }
        }
    } else if released {
        board.dragging = None;
    }
    if let Some(key) = to_start {
        match board.command_now("start", &[key]) {
            Ok(answer) => requests.push(Request::Message(answer.message)),
            Err(problem) => requests.push(Request::Message(problem)),
        }
        requests.push(Request::Repaint);
    }
    // Enter opens the ticket the ring is on, which is the one thing the keyboard can do beyond moving.
    if let Some(id) = open_the_chosen {
        to_open = Some(id);
    }
    if let Some(id) = to_open {
        // **Always the modal**, whatever the window's size. `components::modal` already fits a dialog to the room
        // there is and never lets it grow past the window, so the modal is the one view that works at every
        // size — and it is the only one with the description, the fields and Delete in it. Falling back to the
        // pane's own detail below 1160 points meant a person with a smaller window could not reach a ticket's
        // editor at all.
        if let Err(problem) = board.open_the_modal(id) {
            requests.push(Request::Message(problem));
        }
    }
    requests
}

/// A lane's name, its coloured dot and its count.
///
/// The dot's colour says which lane it is without reading the name, which is what the design image does:
/// New is quiet, In Progress is the modified colour, QA Failed is the unsaved colour and Agent Done is
/// the added colour. All four come from `theme::color`.
fn header(ui: &mut egui::Ui, look: &Look<'_>, lane: Rect, status: Status, count: usize) {
    let painter = ui.painter().clone();
    // Four colours a person can tell apart at nine points across, which is what the picture uses: grey for
    // untouched, red for failed, blue for running and the agent's violet for done. Three of the four are
    // already Quill's; the violet is `color::AGENT`, and the reason it was added is written down beside it.
    let dot = match status {
        Status::New => look.palette.text_dim,
        Status::QaFailed => crate::theme::color::CLOSE,
        Status::InProgress => look.palette.board_accent,
        Status::AgentDone => look.palette.agent,
    };
    let middle = lane.min.y + lane_header(look) / 2.0;
    let centre = Pos2::new(lane.min.x + LANE_INSET + 5.0, middle);
    // The halo round the dot, which is the whole of what makes it read as lit rather than printed. `epaint`
    // has no blur that is not a rectangle, so with the decoration off it is simply the dot.
    if look.chrome.is_recording() {
        // Five points of halo, which is what the reference measures, and at nearly the dot's own strength:
        // at 3.5 and 75% it read as a speck rather than as something lit.
        look.chrome.glow(Rect::from_center_size(centre, Vec2::splat(9.0)), 4.5, dot.gamma_multiply(0.9), 5.0);
        look.chrome.disc(centre, 4.5, crate::services::vello_canvas::Fill::Solid(dot));
    } else {
        painter.circle_filled(centre, 4.5, dot);
    }
    // The lane's name is set with the tracking the stylesheet gives its caption class, `0.14em`, because at
    // this size a run of capitals set solid reads as one word. Spaced with a thin space by hand, since
    // `egui` has no letter spacing of its own.
    let spaced: String = status.label().chars().flat_map(|letter| [letter, '\u{2009}']).collect();
    text(
        &painter,
        Pos2::new(centre.x + 12.0, middle - look.font_size / 2.0),
        spaced.trim_end(),
        look.font_size - 3.0,
        look.palette.text_dim,
    );
    let said = painter.layout_no_wrap(
        count.to_string(),
        egui::FontId::proportional(look.font_size - 2.5),
        look.palette.text_dim,
    );
    // The count sits in a pill pressed into the lane, which is `--e-pressed-sm` and is the one part of the
    // picture that was measured pixel by pixel: six points of dark ramp inside its top left edge and six of
    // pale inside its bottom right.
    let chip = Rect::from_min_size(
        Pos2::new(lane.max.x - LANE_INSET - 40.0, middle - 11.0),
        Vec2::new(40.0, 22.0),
    );
    match look.chrome.is_recording() {
        true => look.chrome.sunken(
            chip,
            10.0,
            look.ground(look.palette.board_well),
            crate::services::vello_canvas::Lift::Small,
        ),
        false => {
            painter.rect(
                chip,
                CornerRadius::same(10),
                look.ground(look.palette.board_well),
                egui::Stroke::new(1.0, look.palette.divider),
                egui::StrokeKind::Inside,
            );
        }
    }
    painter.galley(
        Pos2::new(chip.center().x - said.size().x / 2.0, middle - said.size().y / 2.0),
        said,
        look.palette.text_dim,
    );
}

// **The two listings that used to live here are gone.** `listing` drew the Backlog and the Completed views
// as one flat column of the same cards the lanes hold, and `epics` drew a coloured dot beside a name.
// `task-1771` asked for both to be what the page this board is modelled on has - groups by sprint, rows
// rather than cards, drag and drop between them, and epics you can rename, recolour and delete - which is
// enough of its own thing to be its own file: `components::agent_tasks::listings`.