//! The three views that are not the board: Backlog, Completed and Epics.
//!
//! `task-1771` asks for these to be what the page this board is modelled on has — *"spend time researching
//! each of these tabs, and mirror functionality (drag and drop to rearrange, create epic, etc). Make sure
//! the icons and styling match (except its dark theme)"* — and what was here was a flat column of cards and
//! a list of coloured dots. `ui/src/app/tasks/components/BacklogView.tsx`, `CompletedSprints.tsx` and
//! `EpicsView.tsx` are the three files this is measured against.
//!
//! ## What each of them is, in one sentence
//!
//! **Backlog** groups by *sprint*: every sprint that has not been completed, then the backlog itself, each
//! with its own heading, its own count and its own buttons, and a ticket dragged from one group to another
//! changes which sprint it is in. **Completed** is the same shape with the finished sprints, newest first,
//! each folding shut, and nothing in it can be dragged — a record of what was done is not somewhere to work.
//! **Epics** is a grid of cards: a colour, a name, how many tickets carry it, the seven colours it can be
//! changed to, and Rename and Delete.
//!
//! ## The rows are rows, and that is on purpose
//!
//! A card on the board is 100 points tall and carries a play button, a badge and three counts, because a
//! lane holds a dozen of them and each one is a thing to act on. A listing holds hundreds, and what somebody
//! is doing there is *finding* one — so a row is one line: the key, the priority, the title, the epic, the
//! lane it is in and who has it. That is what the reference draws, and the difference between the two is the
//! difference between a board and a list.
//!
//! ## The drawing decides nothing, and `act` is where the deciding is
//!
//! Every function that **paints** takes a rectangle, paints, and reports what was pressed as a [`Pressed`];
//! not one of them changes the board. [`act`] is the one place a press becomes a change, and it runs once
//! the drawing has finished and the borrow of the board has ended. That is what lets a drag be settled after
//! every group has said where it is — a row cannot know which group the pointer ended up over.
//!
//! [`groups`] and [`epics`] are the provider's own entries rather than components: they draw and then call
//! `act`, which is exactly the shape `agent_chat::pane` and `lanes::show` already have. The distinction that
//! matters is not "this function never changes anything" but "**nothing is changed while the board is being
//! read**", which is what the borrow checker is being used to enforce here.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::{clipped_in, darken, lighten, primary_button, text, PAD};
use crate::services::agent_tasks::model::{Priority, Sprint, SprintStatus, Status, Task};
use crate::services::agent_tasks::{AgentTasks, Group, View, SWATCHES};
use crate::services::plugin_ui::{Look, Request};
use crate::services::vello_canvas::{Fill, Lift};
use crate::theme::icon;

/// One row of a listing, and the gap between two.
///
/// 34 points at the default size, which is a line of text with room round it: the reference's own row is
/// 48 CSS pixels of which 16 is padding. Read through [`row_height`] so a window set to large text gets rows
/// that can hold it, which is `Look::scale`'s rule.
const ROW_AT_DEFAULT: f32 = 34.0;
const ROW_GAP: f32 = 6.0;
/// The strip at the top of a group holding its name, its badge, its count and its buttons.
const GROUP_HEAD_AT_DEFAULT: f32 = 38.0;
/// Between two groups, and the padding inside one.
const GROUP_GAP: f32 = 18.0;
const GROUP_PAD: f32 = 10.0;
/// How round a group and a row are: the board's own `--r-lg` and `--r-md`.
const GROUP_RADIUS: f32 = 16.0;
const ROW_RADIUS: f32 = 10.0;
/// How wide the coloured edge naming a row's epic is, which is what a card already draws.
const EDGE: f32 = 3.0;
/// The strip above the groups, holding `+ New sprint` or the box that names one.
const TOOLBAR_AT_DEFAULT: f32 = 40.0;
/// An epic card in the Epics grid.
const EPIC_CARD_WIDTH: f32 = 300.0;
const EPIC_CARD_HEIGHT: f32 = 132.0;
const EPIC_GAP: f32 = 16.0;
/// How big one colour swatch is, and the gap between two.
const SWATCH: f32 = 18.0;
const SWATCH_GAP: f32 = 8.0;

fn row_height(look: &Look<'_>) -> f32 {
    ROW_AT_DEFAULT * look.scale()
}

fn group_head(look: &Look<'_>) -> f32 {
    GROUP_HEAD_AT_DEFAULT * look.scale()
}

/// What one frame of a listing reported, applied by [`act`] once the drawing is over.
///
/// A value rather than a call, for the reason every component in Quill reports rather than acts: the
/// drawing holds a borrow of the board for the whole of itself, and every one of these changes it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pressed {
    /// A row was clicked, which opens its ticket in the modal.
    pub open: Option<i64>,
    /// A row is being carried, and where the pointer is now.
    pub carrying: Option<(i64, Pos2)>,
    /// The carried row was let go on this frame.
    pub dropped: bool,
    /// A completed sprint's heading was pressed, which folds it.
    pub fold: Option<i64>,
    /// One of a sprint's three buttons.
    pub activate: Option<i64>,
    pub complete: Option<i64>,
    pub ask_about_sprint: Option<Option<i64>>,
    pub delete_sprint: Option<i64>,
    /// The `+ New sprint` button, its field, and the two buttons under it.
    pub name_a_sprint: Option<bool>,
    pub create_sprint: bool,
    /// The Epics view.
    pub create_epic: bool,
    pub choose_colour: Option<String>,
    pub rename_epic: Option<(Option<i64>, String)>,
    pub save_epic_name: Option<i64>,
    pub recolour_epic: Option<(i64, String)>,
    pub ask_about_epic: Option<Option<i64>>,
    pub delete_epic: Option<i64>,
}

/// Apply what a listing reported. The one place any of these views changes the board.
pub fn act(board: &mut AgentTasks, pressed: Pressed) -> Vec<Request> {
    let mut requests = Vec::new();
    let mut say = |problem: String| requests.push(Request::Message(problem));
    // The drag first, because it is the one that runs every frame while a row is in the air and the rest
    // cannot happen at the same time as it.
    match pressed.carrying {
        Some((task, _)) => board.carry(Some(task)),
        None if !pressed.dropped => board.carry(None),
        None => {}
    }
    if pressed.dropped {
        if let Err(problem) = board.drop_the_carried_row() {
            say(problem);
        }
    }
    if let Some(id) = pressed.open {
        if let Err(problem) = board.open_the_modal(id) {
            say(problem);
        }
    }
    if let Some(sprint) = pressed.fold {
        board.toggle_collapsed(sprint);
    }
    if let Some(sprint) = pressed.activate {
        if let Err(problem) = board.command_now("sprint-activate", &[sprint.to_string()]).map(|_| ()) {
            say(problem);
        }
    }
    if let Some(sprint) = pressed.complete {
        if let Err(problem) = board.command_now("sprint-complete", &[sprint.to_string()]).map(|_| ()) {
            say(problem);
        }
    }
    if let Some(asked) = pressed.ask_about_sprint {
        board.ask_about_a_sprint(asked);
    }
    if let Some(sprint) = pressed.delete_sprint {
        board.ask_about_a_sprint(None);
        if let Err(problem) = board.command_now("sprint-delete", &[sprint.to_string()]).map(|_| ()) {
            say(problem);
        }
    }
    if let Some(naming) = pressed.name_a_sprint {
        board.name_a_sprint(naming);
    }
    if pressed.create_sprint {
        let name = board.new_sprint_draft().trim().to_owned();
        if !name.is_empty() {
            if let Err(problem) = board.command_now("new-sprint", &[name]).map(|_| ()) {
                say(problem);
            }
        }
        board.name_a_sprint(false);
    }
    if pressed.create_epic {
        let name = board.new_epic_draft().trim().to_owned();
        if !name.is_empty() {
            let colour = board.new_epic_colour().to_owned();
            // **Made first and coloured after**, because `new-epic` takes a name and nothing else, and
            // giving it a second argument would give one command two shapes. The epic is found again by the
            // name that was just typed, which is what `epic-colour` names one by anyway.
            match board.command_now("new-epic", &[name.clone()]) {
                Ok(_) => {
                    if let Err(problem) = board.command_now("epic-colour", &[name, colour]) {
                        say(problem);
                    }
                    board.new_epic_draft().clear();
                }
                Err(problem) => say(problem),
            }
        }
    }
    if let Some(colour) = pressed.choose_colour {
        board.choose_a_colour(&colour);
    }
    if let Some((epic, name)) = pressed.rename_epic {
        board.rename_epic_from(epic, &name);
    }
    if let Some(epic) = pressed.save_epic_name {
        let name = board.epic_name_draft().trim().to_owned();
        if !name.is_empty() {
            if let Err(problem) = board.command_now("epic-rename", &[epic.to_string(), name]).map(|_| ()) {
                say(problem);
            }
        }
        board.rename_epic_from(None, "");
    }
    if let Some((epic, colour)) = pressed.recolour_epic {
        if let Err(problem) = board.command_now("epic-colour", &[epic.to_string(), colour]).map(|_| ()) {
            say(problem);
        }
    }
    if let Some(asked) = pressed.ask_about_epic {
        board.ask_about_an_epic(asked);
    }
    if let Some(epic) = pressed.delete_epic {
        board.ask_about_an_epic(None);
        if let Err(problem) = board.command_now("epic-delete", &[epic.to_string()]).map(|_| ()) {
            say(problem);
        }
    }
    requests
}

/// The Backlog and the Completed views: a column of groups.
///
/// One function for both, because they are one layout with two differences — what can be done to a group,
/// and whether its rows can be carried — and two copies would be two things to keep in step.
pub fn groups(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>, area: Rect, view: View) -> Vec<Request> {
    let scale = look.scale();
    let finished = view == View::Completed;
    // Copied out, because the drawing wants the board and so does everything the drawing reports. They are a
    // few hundred rows of small values, rebuilt only when the board changes.
    let groups: Vec<Group> = board.groups().to_vec();
    let epics = board.board().epics.clone();
    if groups.is_empty() || groups.iter().all(|group| group.tasks.is_empty() && group.sprint.is_none()) {
        text(
            ui.painter(),
            area.min + Vec2::new(PAD, PAD),
            match finished {
                true => "No completed sprints yet.",
                false => "Nothing in the backlog, and no sprints to put anything in.",
            },
            look.font_size,
            look.palette.text_dim,
        );
        if finished {
            return Vec::new();
        }
    }

    let mut pressed = Pressed::default();
    // The toolbar is the Backlog view's only one: `+ New sprint`, which the Completed view has no use for.
    let toolbar = match finished {
        true => 0.0,
        false => TOOLBAR_AT_DEFAULT * scale + GROUP_GAP * scale,
    };
    if !finished {
        let bar = Rect::from_min_size(
            area.min,
            Vec2::new(area.width(), TOOLBAR_AT_DEFAULT * scale),
        );
        new_sprint_bar(board, ui, look, bar, &mut pressed);
    }

    // How tall the whole column is, so the scroll knows what there is to scroll.
    let mut tall = 0.0;
    for group in &groups {
        tall += group_height(board, group, look, finished) + GROUP_GAP * scale;
    }
    let body = Rect::from_min_max(Pos2::new(area.min.x, area.min.y + toolbar), area.max);
    let down = board.listing_scroll(ui, body, (tall - body.height()).max(0.0));

    let collapsed: Vec<i64> = groups
        .iter()
        .filter_map(|group| group.id())
        .filter(|id| board.is_collapsed(*id))
        .collect();
    let carrying = board.carrying();
    let hovered = board.hovered_group();
    // Cut to the body, so a group scrolled half out of it does not draw over the toolbar above.
    let mut column = ui.new_child(egui::UiBuilder::new().max_rect(body));
    column.set_clip_rect(body);
    look.chrome.clip(body, 0.0);

    let mut pen = body.min.y - down;
    // Where each group ended up, so the one the pointer is over can be found **after** every one of them
    // has been drawn. It cannot be found inside the loop: a row reports that it is being carried as it is
    // drawn, so a group drawn before that row would be asked the question before there was anything to ask
    // about. That is `settle_the_tab_drag`'s own reason, and it is the same shape.
    let mut placed: Vec<(Option<i64>, Rect)> = Vec::with_capacity(groups.len());
    for group in &groups {
        let height = group_height(board, group, look, finished);
        let at = Rect::from_min_size(Pos2::new(body.min.x, pen), Vec2::new(body.width(), height));
        pen += height + GROUP_GAP * scale;
        placed.push((group.id(), at));
        if at.max.y < body.min.y || at.min.y > body.max.y {
            continue;
        }
        let folded = group.id().is_some_and(|id| collapsed.contains(&id));
        let landing = carrying.is_some() && hovered == Some(group.id());
        one_group(
            &mut column,
            look,
            at,
            group,
            GroupLook { epics: &epics, finished, folded, landing, asked: board.sprint_to_delete() },
            &mut pressed,
        );
    }
    // Which group the pointer is over, for a row let go anywhere in it. The **whole** group rather than its
    // rows, which is what the page this is modelled on does: an empty sprint has to be a target, or there
    // would be no way to put the first ticket in one.
    let over: Option<Option<i64>> = pressed
        .carrying
        .map(|(_, at)| at)
        .and_then(|at| placed.iter().find(|(_, rect)| rect.contains(at)).map(|(id, _)| *id));
    look.chrome.unclip();
    if pressed.carrying.is_some() || pressed.dropped {
        board.hover_group(over);
    }
    // The name of what is being carried, under the pointer, which is what the explorer's own row drag draws.
    if let Some((task, at_pointer)) = pressed.carrying {
        let name = groups
            .iter()
            .flat_map(|group| group.tasks.iter())
            .find(|found| found.id == task)
            .map(|found| found.key.clone())
            .unwrap_or_default();
        carried_name(ui, look, area, &name, at_pointer, over.is_some());
    }
    act(board, pressed)
}

/// How tall one group is: its heading, and its rows unless it is folded shut.
fn group_height(board: &AgentTasks, group: &Group, look: &Look<'_>, finished: bool) -> f32 {
    let scale = look.scale();
    let head = group_head(look) + GROUP_PAD * scale;
    if finished && group.id().is_some_and(|id| board.is_collapsed(id)) {
        return head + GROUP_PAD * scale;
    }
    let rows = group.tasks.len().max(1) as f32;
    head + rows * row_height(look) + (rows - 1.0) * ROW_GAP * scale + GROUP_PAD * scale
}

/// Everything about a group that changes how it is drawn, in one value so the argument list stays readable.
#[derive(Debug, Clone, Copy)]
struct GroupLook<'a> {
    /// The epics, so a row can name and colour its own: a `Task` carries an `epic_id` and nothing else, and
    /// the board is what turns one into a name — which is the rule a card on the board already follows.
    epics: &'a [crate::services::agent_tasks::model::Epic],
    finished: bool,
    folded: bool,
    landing: bool,
    asked: Option<i64>,
}

/// One group: a raised panel with a heading and a run of rows.
fn one_group(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    group: &Group,
    how: GroupLook<'_>,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let radius = GROUP_RADIUS * scale;
    // **The panel a group sits on.** Raised, which is what a lane is, so a listing and a board are plainly
    // the same board seen two ways. A group a row would land in is outlined in the accent, which is what
    // `.sprint-group__body--drop` does and is the one thing on this page that says a drop will work.
    if look.chrome.is_recording() {
        look.chrome.raised(area, radius, Fill::Solid(look.ground(look.palette.board_lane)), Lift::Small);
    } else {
        ui.painter().rect(
            area,
            CornerRadius::same(radius as u8),
            look.ground(look.palette.board_lane),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    if how.landing {
        ui.painter().rect_stroke(
            area,
            CornerRadius::same(radius as u8),
            Stroke::new(2.0, look.palette.board_accent),
            egui::StrokeKind::Inside,
        );
    }

    let head = Rect::from_min_size(
        Pos2::new(area.min.x + GROUP_PAD * scale, area.min.y + GROUP_PAD * scale),
        Vec2::new(area.width() - GROUP_PAD * 2.0 * scale, group_head(look)),
    );
    group_heading(ui, look, head, group, how, pressed);
    if how.finished && how.folded {
        return;
    }

    let mut pen = head.max.y;
    if group.tasks.is_empty() {
        text(
            ui.painter(),
            Pos2::new(head.min.x + 4.0 * scale, pen + (row_height(look) - look.font_size) / 2.0),
            match group.sprint.is_some() {
                true => "Nothing here yet — drag a ticket in",
                false => "No backlog tickets — drag one here to take it out of its sprint",
            },
            look.font_size - 1.0,
            look.palette.text_faint,
        );
        return;
    }
    for task in &group.tasks {
        let at = Rect::from_min_size(
            Pos2::new(head.min.x, pen),
            Vec2::new(head.width(), row_height(look)),
        );
        pen += row_height(look) + ROW_GAP * scale;
        row(ui, look, at, task, how.epics, !how.finished, pressed);
    }
}

/// A group's heading: the chevron a completed one folds by, its name, its badge, its count and its buttons.
fn group_heading(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    group: &Group,
    how: GroupLook<'_>,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let painter = ui.painter().clone();
    let middle = area.center().y;
    let mut pen = area.min.x;

    // **The heading carries the group's name**, so a test and an agent can find a group by what it is
    // called rather than by counting rows. Added first and sensing nothing, so every control put on the
    // heading after it takes back the points it covers - the order `components::dock::handle` documents.
    let named = ui.interact(
        area,
        ui.id().with(("agent-tasks-group", group.id())),
        Sense::hover(),
    );
    named.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), group.name())
    });

    // A completed group folds, which is what `CompletedSprints.tsx` does — a finished fortnight of sixty
    // tickets is a heading most of the time. Nothing folds in the Backlog view, because there a group is
    // somewhere to drop a ticket and a folded one could not be dropped on.
    if how.finished {
        if let Some(id) = group.id() {
            let hit = Rect::from_min_size(
                Pos2::new(area.min.x - 4.0 * scale, area.min.y),
                Vec2::new(area.width(), area.height()),
            );
            let response =
                ui.interact(hit, ui.id().with(("agent-tasks-fold", id)), Sense::click());
            icon::disclosure_at(
                &painter,
                Pos2::new(pen + 6.0 * scale, middle),
                !how.folded,
                look.palette.text_dim,
                scale,
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    !how.folded,
                    format!("Fold {}", group.name()),
                )
            });
            if response.clicked() {
                pressed.fold = Some(id);
            }
        }
        pen += 18.0 * scale;
    }

    // The name, set bold, which is `.sprint-group__name`.
    let name = painter.layout_no_wrap(
        group.name().to_owned(),
        egui::FontId::new(look.font_size + 1.0, egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into())),
        look.palette.text_strong,
    );
    let widest = (area.width() * 0.45).max(40.0);
    if name.size().x <= widest {
        painter.galley(Pos2::new(pen, middle - name.size().y / 2.0), name.clone(), look.palette.text_strong);
        pen += name.size().x + 10.0 * scale;
    } else {
        clipped_in(
            &painter,
            Pos2::new(pen, middle - look.font_size * 0.7),
            group.name(),
            egui::FontId::new(look.font_size + 1.0, egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into())),
            look.palette.text_strong,
            widest,
            1,
        );
        pen += widest + 10.0 * scale;
    }

    // The badge: `ACTIVE`, `planned` or `COMPLETED`, which is `.sprint-group__badge`.
    if let Some(sprint) = group.sprint.as_ref() {
        pen += badge(&painter, look, Pos2::new(pen, middle), sprint) + 10.0 * scale;
    }
    // And how many tickets are in it.
    pen += text(
        &painter,
        Pos2::new(pen, middle - look.font_size * 0.6),
        &super::card::plural(group.tasks.len() as i64, "task"),
        look.font_size - 1.0,
        look.palette.text_dim,
    );
    let _ = pen;

    // The buttons, from the right. A control that cannot apply is absent, which is Quill's rule and is why
    // the backlog group has none at all: it is not a sprint and there is nothing to activate, complete or
    // delete about it.
    let Some(sprint) = group.sprint.as_ref() else {
        return;
    };
    if how.finished {
        return;
    }
    let mut right = area.max.x;
    // **The id as well as the name.** Two sprints may be called the same thing — the schema allows it and
    // the commands accept an id for exactly that reason — and two controls called `Complete September` are
    // two controls with one name and one `egui` id, which is the case `CLAUDE.md`'s naming rule exists to
    // stop. Found by the `task-1771` review. The name comes first because it is what a person reads.
    let about = |what: &str| format!("{what} {} ({})", sprint.name, sprint.id);
    if how.asked == Some(sprint.id) {
        // Asked rather than done. The two answers are drawn where the one button was.
        let no = quiet_button_named(ui, look, right, middle, "Cancel", &about("Keep"), false);
        right = no.0;
        if no.1 {
            pressed.ask_about_sprint = Some(None);
        }
        let yes = quiet_button_named(
            ui,
            look,
            right - 6.0 * scale,
            middle,
            "Delete sprint?",
            &about("Really delete"),
            true,
        );
        right = yes.0;
        if yes.1 {
            pressed.delete_sprint = Some(sprint.id);
        }
        let _ = right;
        return;
    }
    if sprint.status != SprintStatus::Active {
        let hit = quiet_button_named(ui, look, right, middle, "Delete", &about("Delete"), true);
        right = hit.0;
        if hit.1 {
            pressed.ask_about_sprint = Some(Some(sprint.id));
        }
        right -= 6.0 * scale;
    }
    let complete =
        quiet_button_named(ui, look, right, middle, "Complete", &about("Complete"), false);
    right = complete.0;
    if complete.1 {
        pressed.complete = Some(sprint.id);
    }
    if sprint.status != SprintStatus::Active {
        right -= 6.0 * scale;
        let make =
            quiet_button_named(ui, look, right, middle, "Make active", &about("Activate"), false);
        if make.1 {
            pressed.activate = Some(sprint.id);
        }
    }
}

/// A sprint's status, as the small capitalised chip the reference draws.
fn badge(painter: &egui::Painter, look: &Look<'_>, at: Pos2, sprint: &Sprint) -> f32 {
    let (said, tint) = match sprint.status {
        SprintStatus::Active => ("ACTIVE", look.palette.added),
        SprintStatus::Planned => ("PLANNED", look.palette.text_dim),
        SprintStatus::Completed => ("COMPLETED", look.palette.text_faint),
    };
    let size = look.font_size - 3.0;
    let galley = painter.layout_no_wrap(said.to_owned(), egui::FontId::proportional(size), tint);
    let chip = Rect::from_min_size(
        Pos2::new(at.x, at.y - galley.size().y / 2.0 - 3.0),
        galley.size() + Vec2::new(16.0, 6.0),
    );
    painter.rect(
        chip,
        CornerRadius::same((chip.height() / 2.0) as u8),
        egui::Color32::TRANSPARENT,
        Stroke::new(1.0, tint.gamma_multiply(0.55)),
        egui::StrokeKind::Inside,
    );
    painter.galley(Pos2::new(chip.min.x + 8.0, at.y - galley.size().y / 2.0), galley, tint);
    chip.width()
}

/// A small text button that lives in a heading, laid out from the right.
///
/// Answers the left edge it took, so the next one along can be placed against it, and whether it was
/// pressed. `danger` draws it in the one red the board already has.
fn quiet_button(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    right: f32,
    middle: f32,
    label: &str,
    danger: bool,
) -> (f32, bool) {
    quiet_button_named(ui, look, right, middle, label, label, danger)
}

/// The same, with a name of its own.
///
/// **No two controls in one window may share a name**, which is `design/style-guide.md`'s rule and what a
/// test and an agent find a control by. Three sprints each drawing a button that says `Complete` are three
/// controls called the same thing, so what is *written* on the button stays short and what it is *called*
/// carries the thing it is about.
fn quiet_button_named(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    right: f32,
    middle: f32,
    label: &str,
    name: &str,
    danger: bool,
) -> (f32, bool) {
    let scale = look.scale();
    let tint = match danger {
        true => crate::theme::color::close(),
        false => look.palette.text_control,
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(look.font_size - 2.0),
        tint,
    );
    let area = Rect::from_min_size(
        Pos2::new(right - galley.size().x - 16.0 * scale, middle - 11.0 * scale),
        Vec2::new(galley.size().x + 16.0 * scale, 22.0 * scale),
    );
    let response = ui.interact(area, ui.id().with(("agent-tasks-quiet", name)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            area,
            CornerRadius::same((area.height() / 2.0) as u8),
            egui::Color32::from_white_alpha(16),
        );
    }
    ui.painter().galley(
        Pos2::new(area.center().x - galley.size().x / 2.0, middle - galley.size().y / 2.0),
        galley,
        tint,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    (area.min.x, response.clicked())
}

/// One ticket, as a row: the key, the priority, the title, the epic, the lane and who has it.
fn row(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    task: &Task,
    epics: &[crate::services::agent_tasks::model::Epic],
    draggable: bool,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let sense = match draggable {
        true => Sense::click_and_drag(),
        false => Sense::click(),
    };
    let response = ui.interact(area, ui.id().with(("agent-tasks-row", task.id)), sense);
    let radius = ROW_RADIUS * scale;
    if look.chrome.is_recording() {
        look.chrome.raised(area, radius, Fill::Solid(look.ground(look.palette.board_card)), Lift::Small);
    } else {
        ui.painter().rect(
            area,
            CornerRadius::same(radius as u8),
            look.ground(look.palette.board_card),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(radius as u8), egui::Color32::from_white_alpha(10));
    }
    // The coloured edge naming the epic, which is `.row-task`'s own `border-left-color` and what a card on
    // the board already draws.
    let epic = task.epic_id.and_then(|id| epics.iter().find(|epic| epic.id == id));
    if let Some(colour) = epic.and_then(|epic| crate::services::plugins::colour(&epic.color)) {
        ui.painter().rect_filled(
            Rect::from_min_size(area.min, Vec2::new(EDGE * scale, area.height())),
            CornerRadius { nw: radius as u8, sw: radius as u8, ne: 0, se: 0 },
            egui::Color32::from_rgb(colour.r, colour.g, colour.b),
        );
    }

    let painter = ui.painter().clone();
    let middle = area.center().y;
    let mut pen = area.min.x + 12.0 * scale;
    // The key, in the code face, which is what makes a column of them line up.
    let key = painter.layout_no_wrap(
        task.key.clone(),
        egui::FontId::monospace(look.font_size - 2.0),
        look.palette.text_dim,
    );
    painter.galley(Pos2::new(pen, middle - key.size().y / 2.0), key.clone(), look.palette.text_dim);
    pen += key.size().x + 10.0 * scale;
    // The priority, as the chevron a card already uses: up for high, down for low, nothing for medium.
    if task.priority != Priority::Medium {
        let up = task.priority == Priority::High;
        let tint = match up {
            true => crate::theme::color::close(),
            false => look.palette.text_faint,
        };
        chevron(&painter, Pos2::new(pen + 4.0 * scale, middle), up, tint, scale);
        pen += 14.0 * scale;
    }

    // From the right: who has it, which lane it is in, and its epic. Placed first so the title knows how
    // much room is left, which is the rule the board's own header follows.
    let mut right = area.max.x - 10.0 * scale;
    right -= avatar(&painter, look, Pos2::new(right, middle), task);
    right -= 10.0 * scale;
    right -= lane_chip(&painter, look, Pos2::new(right, middle), task.status);
    if let Some(epic) = epic {
        right -= 10.0 * scale;
        right -= epic_chip(&painter, look, Pos2::new(right, middle), &epic.name, Some(&epic.color));
    }

    // The title takes what is left, on one line, cut short rather than wrapped.
    let room = (right - pen - 12.0 * scale).max(20.0);
    clipped_in(
        &painter,
        Pos2::new(pen, middle - look.font_size * 0.7),
        &task.display_title(),
        egui::FontId::proportional(look.font_size),
        look.palette.text,
        room,
        1,
    );

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("{} {}", task.key, task.display_title()),
        )
    });
    if response.clicked() {
        pressed.open = Some(task.id);
    }
    if draggable && (response.dragged() || response.drag_stopped()) {
        if let Some(at) = response.interact_pointer_pos() {
            pressed.carrying = Some((task.id, at));
            pressed.dropped = response.drag_stopped();
        }
    }
}

/// The lane a ticket is in, as a chip. Answers how wide it drew, right aligned at `at`.
fn lane_chip(painter: &egui::Painter, look: &Look<'_>, at: Pos2, status: Status) -> f32 {
    let tint = match status {
        Status::New => look.palette.text_dim,
        Status::QaFailed => look.palette.unsaved,
        Status::InProgress => look.palette.modified,
        Status::AgentDone => look.palette.added,
    };
    let galley = painter.layout_no_wrap(
        status.label().to_owned(),
        egui::FontId::proportional(look.font_size - 3.0),
        tint,
    );
    painter.galley(
        Pos2::new(at.x - galley.size().x, at.y - galley.size().y / 2.0),
        galley.clone(),
        tint,
    );
    galley.size().x
}

/// The epic's name in its own colour, which is the chip a card already draws.
fn epic_chip(painter: &egui::Painter, look: &Look<'_>, at: Pos2, name: &str, colour: Option<&str>) -> f32 {
    let tint = colour
        .and_then(crate::services::plugins::colour)
        .map(|found| egui::Color32::from_rgb(found.r, found.g, found.b))
        .unwrap_or(look.palette.text_dim);
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(look.font_size - 3.0),
        tint,
    );
    let chip = Rect::from_min_size(
        Pos2::new(at.x - galley.size().x - 14.0, at.y - galley.size().y / 2.0 - 3.0),
        galley.size() + Vec2::new(14.0, 6.0),
    );
    painter.rect_filled(chip, CornerRadius::same((chip.height() / 2.0) as u8), tint.gamma_multiply(0.16));
    painter.galley(Pos2::new(chip.min.x + 7.0, at.y - galley.size().y / 2.0), galley, tint);
    chip.width()
}

/// Who the ticket is assigned to, as the small disc a card draws.
fn avatar(painter: &egui::Painter, look: &Look<'_>, at: Pos2, task: &Task) -> f32 {
    let radius = look.font_size * 0.62;
    let ground = match task.assignee {
        crate::services::agent_tasks::model::Assignee::Human => look.palette.control,
        _ => look.palette.agent,
    };
    painter.circle_filled(Pos2::new(at.x - radius, at.y), radius, ground);
    let letter = task.assignee.name().chars().next().unwrap_or('?').to_uppercase().to_string();
    let galley = painter.layout_no_wrap(
        letter,
        egui::FontId::proportional(look.font_size - 4.0),
        look.palette.text_strong,
    );
    painter.galley(
        Pos2::new(at.x - radius - galley.size().x / 2.0, at.y - galley.size().y / 2.0),
        galley,
        look.palette.text_strong,
    );
    radius * 2.0
}

/// The priority chevron, at this board's scale.
fn chevron(painter: &egui::Painter, centre: Pos2, up: bool, tint: egui::Color32, scale: f32) {
    let w = 4.0 * scale;
    let h = 2.6 * scale;
    let points = match up {
        true => vec![
            Pos2::new(centre.x - w, centre.y + h),
            Pos2::new(centre.x + w, centre.y + h),
            Pos2::new(centre.x, centre.y - h),
        ],
        false => vec![
            Pos2::new(centre.x - w, centre.y - h),
            Pos2::new(centre.x + w, centre.y - h),
            Pos2::new(centre.x, centre.y + h),
        ],
    };
    painter.add(egui::Shape::convex_polygon(points, tint, Stroke::NONE));
}

/// The name of the ticket being carried, drawn under the pointer.
///
/// The explorer's own row drag draws exactly this, and for the same reason: without it a drag over a long
/// list is a pointer moving over rows with nothing to say what is in the air.
fn carried_name(ui: &egui::Ui, look: &Look<'_>, area: Rect, name: &str, at: Pos2, welcome: bool) {
    let painter = ui.painter_at(area.expand(4.0));
    let tint = match welcome {
        true => look.palette.text_strong,
        false => look.palette.text_faint,
    };
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::monospace(look.font_size - 2.0), tint);
    let box_rect =
        Rect::from_min_size(at + Vec2::new(12.0, 6.0), galley.size() + Vec2::new(14.0, 8.0));
    painter.rect(
        box_rect,
        CornerRadius::same(5),
        look.palette.menu,
        Stroke::new(1.0, match welcome {
            true => look.palette.board_accent,
            false => look.palette.control_border,
        }),
        egui::StrokeKind::Inside,
    );
    painter.galley(box_rect.min + Vec2::new(7.0, 4.0), galley, tint);
}

/// The strip above the Backlog view's groups: `+ New sprint`, or the box that names one.
fn new_sprint_bar(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let height = 30.0 * scale;
    let middle = area.center().y;
    if !board.naming_a_sprint() {
        let width = (120.0 * scale).min(area.width());
        let at = Rect::from_min_size(
            Pos2::new(area.max.x - width, middle - height / 2.0),
            Vec2::new(width, height),
        );
        let hit = quiet_button(ui, look, at.max.x, at.center().y, "+ New sprint", false);
        if hit.1 {
            pressed.name_a_sprint = Some(true);
        }
        return;
    }
    // The field, with Create and Cancel beside it — which is what `BacklogView.tsx` swaps the button for.
    let mut right = area.max.x;
    let cancel = quiet_button(ui, look, right, middle, "Cancel", false);
    right = cancel.0 - 6.0 * scale;
    if cancel.1 {
        pressed.name_a_sprint = Some(false);
    }
    let create = quiet_button(ui, look, right, middle, "Create", false);
    right = create.0 - 8.0 * scale;
    if create.1 {
        pressed.create_sprint = true;
    }
    let width = (220.0 * scale).min((right - area.min.x).max(40.0));
    let field = Rect::from_min_size(
        Pos2::new(right - width, middle - height / 2.0),
        Vec2::new(width, height),
    );
    if plain_field(ui, look, field, "New sprint name", "Sprint name…", board.new_sprint_draft()) {
        pressed.create_sprint = true;
    }
}

/// A one-line text box drawn the way the board draws a well. Answers whether Enter was pressed in it.
fn plain_field(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    name: &str,
    hint: &str,
    value: &mut String,
) -> bool {
    if look.chrome.is_recording() {
        look.chrome.sunken(area, area.height() / 2.0, look.ground(look.palette.board_well), Lift::Small);
    } else {
        ui.painter().rect(
            area,
            CornerRadius::same((area.height() / 2.0) as u8),
            look.palette.field,
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let field_id = ui.id().with(("agent-tasks-listing-field", name));
    let inner = crate::components::controls::field_takes_the_whole_rectangle(ui, area, 14.0, field_id);
    let response = ui
        .push_id(name, |ui| {
            ui.put(
                inner,
                egui::TextEdit::singleline(value)
                    .id(field_id)
                    .frame(egui::Frame::NONE)
                    .hint_text(egui::RichText::new(hint).color(look.palette.text_faint))
                    .font(egui::FontId::proportional(look.font_size - 1.0))
                    .text_color(look.palette.text),
            )
        })
        .inner;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
    super::enter_was_used_and_pressed(ui, &response)
}

/// The Epics view: a bar that makes one, and a grid of cards.
pub fn epics(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Request> {
    let scale = look.scale();
    let mut pressed = Pressed::default();
    let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), TOOLBAR_AT_DEFAULT * scale));
    new_epic_bar(board, ui, look, bar, &mut pressed);

    let epics = board.board().epics.clone();
    let body = Rect::from_min_max(
        Pos2::new(area.min.x, bar.max.y + GROUP_GAP * scale),
        area.max,
    );
    if epics.is_empty() {
        text(
            ui.painter(),
            body.min + Vec2::new(4.0, 4.0),
            "No epics yet. Name one above and press Add epic.",
            look.font_size,
            look.palette.text_dim,
        );
        return act(board, pressed);
    }
    // A grid as wide as the room allows, which is what `.epics-grid` is: a card is a fixed width and the
    // number across follows from the pane rather than being chosen.
    // **Never wider than the page it is on.** A card is 300 points at the default size and three times
    // that at the largest zoom, so in a narrow pane it would be drawn off the right hand edge with its
    // Rename and Delete beyond anything the pointer can reach. Found by the `task-1771` review: a zoom is
    // only worth having if what it draws stays usable at both ends of it.
    let card = Vec2::new((EPIC_CARD_WIDTH * scale).min(body.width()), EPIC_CARD_HEIGHT * scale);
    let across = (((body.width() + EPIC_GAP * scale) / (card.x + EPIC_GAP * scale)).floor() as usize).max(1);
    let rows = epics.len().div_ceil(across);
    let tall = rows as f32 * (card.y + EPIC_GAP * scale);
    let down = board.listing_scroll(ui, body, (tall - body.height()).max(0.0));

    let renaming = board.renaming_epic();
    let asked = board.epic_to_delete();
    let counts: Vec<i64> = epics.iter().map(|epic| board.epic_count(epic.id)).collect();
    let mut grid = ui.new_child(egui::UiBuilder::new().max_rect(body));
    grid.set_clip_rect(body);
    look.chrome.clip(body, 0.0);
    for (index, epic) in epics.iter().enumerate() {
        let column = index % across;
        let row_of = index / across;
        let at = Rect::from_min_size(
            Pos2::new(
                body.min.x + column as f32 * (card.x + EPIC_GAP * scale),
                body.min.y + row_of as f32 * (card.y + EPIC_GAP * scale) - down,
            ),
            card,
        );
        if at.max.y < body.min.y || at.min.y > body.max.y {
            continue;
        }
        epic_card(
            &mut grid,
            look,
            at,
            epic,
            counts[index],
            renaming == Some(epic.id),
            asked == Some(epic.id),
            &mut pressed,
        );
    }
    look.chrome.unclip();
    // The name being typed into a card is borrowed mutably, so it is drawn after the loop rather than
    // inside it: the loop holds the epics, which came off the board.
    if let Some(id) = renaming {
        if let Some(index) = epics.iter().position(|epic| epic.id == id) {
            let column = index % across;
            let row_of = index / across;
            let at = Rect::from_min_size(
                Pos2::new(
                    body.min.x + column as f32 * (card.x + EPIC_GAP * scale),
                    body.min.y + row_of as f32 * (card.y + EPIC_GAP * scale) - down,
                ),
                card,
            );
            let field = Rect::from_min_size(
                Pos2::new(at.min.x + 44.0 * scale, at.min.y + 12.0 * scale),
                Vec2::new(at.width() - 90.0 * scale, 26.0 * scale),
            );
            let mut naming = ui.new_child(egui::UiBuilder::new().max_rect(body));
            naming.set_clip_rect(body);
            if plain_field(&mut naming, look, field, "Epic name", "Name…", board.epic_name_draft()) {
                pressed.save_epic_name = Some(id);
            }
        }
    }
    act(board, pressed)
}

/// The strip above the Epics grid: a name, the seven colours, and `+ Add epic`.
fn new_epic_bar(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let height = 30.0 * scale;
    let middle = area.center().y;
    let button = Vec2::new((120.0 * scale).min(area.width() * 0.4), height + 4.0 * scale);
    let add = Rect::from_min_size(
        Pos2::new(area.max.x - button.x, middle - button.y / 2.0),
        button,
    );
    if primary_button(ui, look, add, "+ Add epic") {
        pressed.create_epic = true;
    }
    // The seven swatches, in front of the button, squeezed together before they run past the field. See
    // the same rule on a card below.
    let chosen = board.new_epic_colour().to_owned();
    let room = (add.min.x - 12.0 * scale - area.min.x).max(0.0) * 0.5;
    let step = (room / SWATCHES.len() as f32).min(SWATCH * scale + SWATCH_GAP * scale).max(4.0);
    let size = (SWATCH * scale).min(step - 2.0).max(6.0);
    let swatches = SWATCHES.len() as f32 * step;
    let mut pen = add.min.x - 12.0 * scale - swatches;
    for colour in SWATCHES {
        let at = Rect::from_center_size(Pos2::new(pen + size / 2.0, middle), Vec2::splat(size));
        if swatch(ui, look, at, colour, chosen == **colour, "new epic") {
            pressed.choose_colour = Some((*colour).to_owned());
        }
        pen += step;
    }
    let width = (240.0 * scale).min((add.min.x - 12.0 - swatches - area.min.x).max(40.0));
    let field = Rect::from_min_size(Pos2::new(area.min.x, middle - height / 2.0), Vec2::new(width, height));
    if plain_field(ui, look, field, "New epic name", "New epic name…", board.new_epic_draft()) {
        pressed.create_epic = true;
    }
}

/// One colour a person can press. A round disc, ringed when it is the chosen one.
fn swatch(ui: &mut egui::Ui, look: &Look<'_>, area: Rect, colour: &str, chosen: bool, what: &str) -> bool {
    let Some(found) = crate::services::plugins::colour(colour) else {
        return false;
    };
    let tint = egui::Color32::from_rgb(found.r, found.g, found.b);
    let response =
        ui.interact(area, ui.id().with(("agent-tasks-swatch", what, colour)), Sense::click());
    let radius = area.width() / 2.0;
    if look.chrome.is_recording() {
        look.chrome.disc(area.center(), radius, Fill::diagonal(area, lighten(tint, 0.06), darken(tint, 0.1)));
    } else {
        ui.painter().circle_filled(area.center(), radius, tint);
    }
    if chosen {
        ui.painter().circle_stroke(
            area.center(),
            radius + 2.5,
            Stroke::new(1.6, look.palette.text_strong),
        );
    } else if response.hovered() {
        ui.painter().circle_filled(area.center(), radius, egui::Color32::from_white_alpha(24));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            chosen,
            format!("{what} colour {colour}"),
        )
    });
    response.clicked()
}

/// One epic: its colour, its name, how many tickets carry it, its seven colours, and two buttons.
fn epic_card(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    epic: &crate::services::agent_tasks::model::Epic,
    count: i64,
    renaming: bool,
    asked: bool,
    pressed: &mut Pressed,
) {
    let scale = look.scale();
    let radius = GROUP_RADIUS * scale;
    if look.chrome.is_recording() {
        look.chrome.raised(area, radius, Fill::Solid(look.ground(look.palette.board_card)), Lift::Small);
    } else {
        ui.painter().rect(
            area,
            CornerRadius::same(radius as u8),
            look.ground(look.palette.board_card),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let painter = ui.painter().clone();
    let inset = 12.0 * scale;

    // The heading: a rounded square of the epic's colour, its name, and its count at the right.
    let mark = Rect::from_min_size(
        Pos2::new(area.min.x + inset, area.min.y + inset),
        Vec2::splat(26.0 * scale),
    );
    if let Some(found) = crate::services::plugins::colour(&epic.color) {
        let tint = egui::Color32::from_rgb(found.r, found.g, found.b);
        if look.chrome.is_recording() {
            look.chrome.raised(
                mark,
                8.0 * scale,
                Fill::diagonal(mark, lighten(tint, 0.06), darken(tint, 0.1)),
                Lift::Small,
            );
        } else {
            painter.rect_filled(mark, CornerRadius::same((8.0 * scale) as u8), tint);
        }
    }
    let count_said = painter.layout_no_wrap(
        count.to_string(),
        egui::FontId::monospace(look.font_size - 1.0),
        look.palette.text_dim,
    );
    painter.galley(
        Pos2::new(area.max.x - inset - count_said.size().x, mark.center().y - count_said.size().y / 2.0),
        count_said.clone(),
        look.palette.text_dim,
    );
    // The name is drawn unless it is being typed into, in which case the field is drawn over it by the
    // caller — a text box needs a mutable borrow of the draft and this function holds the epic.
    if !renaming {
        clipped_in(
            &painter,
            Pos2::new(mark.max.x + 10.0 * scale, mark.center().y - look.font_size * 0.75),
            &epic.name,
            egui::FontId::new(
                look.font_size + 1.0,
                egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into()),
            ),
            look.palette.text_strong,
            (area.width() - inset * 2.0 - 36.0 * scale - count_said.size().x - 10.0).max(20.0),
            1,
        );
    }

    // The seven colours it can be changed to, **squeezed together rather than run off the card**: at a
    // large zoom in a narrow pane seven discs are wider than the card, and a colour drawn past its own edge
    // is a colour nobody can press. The gap gives way first and the discs themselves after it, which is what
    // a row of marks can afford to do and a word cannot.
    let room = area.width() - inset * 2.0;
    let step = (room / SWATCHES.len() as f32).min(SWATCH * scale + SWATCH_GAP * scale);
    let size = (SWATCH * scale).min(step - 2.0).max(6.0);
    let mut pen = area.min.x + inset;
    let swatch_y = mark.max.y + 16.0 * scale;
    for colour in SWATCHES {
        let at = Rect::from_center_size(Pos2::new(pen + size / 2.0, swatch_y), Vec2::splat(size));
        if swatch(ui, look, at, colour, epic.color.eq_ignore_ascii_case(colour), &epic.name) {
            pressed.recolour_epic = Some((epic.id, (*colour).to_owned()));
        }
        pen += step;
    }

    // Rename, and Delete — which asks first. `general` cannot be deleted, which is the browser board's own
    // rule: it is the epic every ticket falls back to.
    let middle = area.max.y - inset - 11.0 * scale;
    let mut left = area.min.x + inset;
    let label = match renaming {
        true => "Save",
        false => "Rename",
    };
    let galley_width = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(look.font_size - 2.0),
            look.palette.text_control,
        )
        .size()
        .x
        + 16.0 * scale;
    let rename = quiet_button_named(
        ui,
        look,
        left + galley_width,
        middle,
        label,
        &format!("{label} {}", epic.name),
        false,
    );
    left = left + galley_width + 8.0 * scale;
    if rename.1 {
        match renaming {
            true => pressed.save_epic_name = Some(epic.id),
            false => pressed.rename_epic = Some((Some(epic.id), epic.name.clone())),
        }
    }
    if epic.name == "general" {
        return;
    }
    if asked {
        let width = ui
            .painter()
            .layout_no_wrap(
                "Delete?".to_owned(),
                egui::FontId::proportional(look.font_size - 2.0),
                look.palette.text_control,
            )
            .size()
            .x
            + 16.0 * scale;
        let yes = quiet_button_named(
            ui,
            look,
            left + width,
            middle,
            "Delete?",
            &format!("Really delete {}", epic.name),
            true,
        );
        if yes.1 {
            pressed.delete_epic = Some(epic.id);
        }
        let cancel = quiet_button_named(
            ui,
            look,
            area.max.x - inset,
            middle,
            "Cancel",
            &format!("Keep {}", epic.name),
            false,
        );
        if cancel.1 {
            pressed.ask_about_epic = Some(None);
        }
        return;
    }
    let delete = quiet_button_named(
        ui,
        look,
        area.max.x - inset,
        middle,
        "Delete",
        &format!("Delete {}", epic.name),
        true,
    );
    if delete.1 {
        pressed.ask_about_epic = Some(Some(epic.id));
    }
}

