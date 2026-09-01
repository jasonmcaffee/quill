//! Drawing the Agent-Tasks board.
//!
//! Nothing here decides anything. Which lane a card is in, where a drag lands and what a search matches
//! are all `services::agent_tasks`, and this file paints them and reports what was pressed. That is the
//! division every component in Quill makes, and it is what lets the board be tested with no window.
//!
//! ## The look is the window's, and none of it is chosen here
//!
//! Every colour comes from the `Look` the provider was handed, which is `theme::color` with the opacity
//! setting applied. The fonts are the settings' fonts and a row is `look.row_height`. So changing the
//! font size or the transparency in `Settings -> Appearance` changes the board in the same frame, and
//! there is no colour in this file that a plugin chose. The one exception is an epic's own colour, which
//! comes from the data and is confined to a card's left edge and its chip, the same allowance the file
//! icons already have.

pub(crate) mod card;
mod description;
pub(crate) mod detail;
mod lanes;
mod settings_page;
pub mod ticket_modal;

use egui::{Pos2, Rect, Vec2};

use crate::services::agent_tasks::{AgentTasks, View};
use crate::services::plugin_ui::{Look, Request};

/// How tall the strip holding the sprint's name, the search box and `+ Add Task` is.
///
/// **One row.** It was two, because the four view buttons were in it and they did not fit beside the sprint's
/// name at a pane's width. They are down the rail now, which is what the reference does and what §8.5 of the
/// design describes.
///
/// Read through `header_height` so a window set to large text gets a header that can hold it. See
/// `Look::scale`.
const HEADER_AT_DEFAULT: f32 = 62.0;

/// How tall the board's own header is in this window.
fn header_height(look: &Look<'_>) -> f32 {
    HEADER_AT_DEFAULT * look.scale()
}

/// How tall a contributed pane's own header is: the strip that names it, closes it and drags it.
///
/// 28 points, which is what a list row is and what the explorer's footer is, so a panel's header is the
/// same height wherever it is.
pub const PANE_HEADER: f32 = 28.0;

/// What a pane's header reported this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct HeaderOutcome {
    /// The panel is in the air, or was right clicked. `app::dock` is what acts on it.
    pub grab: crate::components::dock::Grab,
    /// The close cross was pressed.
    pub closed: bool,
}

/// A contributed pane's header: its name, how many things are in it, and a cross that closes it.
///
/// **The whole strip is the drag handle**, which is what `components::dock::handle` makes it and what every
/// other panel in Quill already has: the four drop bands, the strong rectangle showing where it would land
/// and the `Move to` menu on a right click all come with that one call. A pane with no header was a pane
/// that could not be moved and could only be put away from the rail.
pub fn pane_header(
    ui: &mut egui::Ui,
    area: Rect,
    label: &str,
    count: Option<&str>,
    panel: crate::app::dock::Panel,
    opacity: f32,
) -> HeaderOutcome {
    let mut outcome = HeaderOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0, crate::theme::faded(crate::theme::color::TOOLBAR, opacity));
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(area.min.x, area.max.y - 1.0), area.max),
        0,
        crate::theme::color::DIVIDER,
    );
    let mut pen = area.min.x + 10.0;
    pen += text(
        &painter,
        Pos2::new(pen, area.center().y - 7.0),
        label,
        12.5,
        crate::theme::color::TEXT_CONTROL,
    );
    if let Some(count) = count {
        text(
            &painter,
            Pos2::new(pen + 8.0, area.center().y - 6.5),
            count,
            11.5,
            crate::theme::color::TEXT_DIM,
        );
    }
    // **The handle first and the cross after it.** egui gives a pointer to the *last* widget that wanted it,
    // and the handle covers the whole strip, so a cross added first was a cross the handle swallowed. That is
    // the order `components::dock::handle`'s own documentation asks for, and it is why every other panel's
    // header adds its controls after the handle.
    outcome.grab = crate::components::dock::handle(ui, area, panel);
    let cross = Rect::from_center_size(
        Pos2::new(area.max.x - 14.0, area.center().y),
        Vec2::splat(18.0),
    );
    outcome.closed = crate::components::controls::icon_button(
        ui,
        cross,
        &format!("Close {label}"),
        crate::theme::icon::cross,
    );
    outcome
}
/// The gap round everything, which is the gap the explorer already leaves.
const PAD: f32 = 8.0;

/// The pane in the rail: the header, then either the lanes or one ticket.
///
/// A pane can be 420 points wide down the right hand side of the window, so the detail **replaces** the
/// lanes here rather than opening beside them or in a modal: a modal inside a 420 point column would be
/// a modal wider than its parent.
pub fn pane(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    // **The ground is the window's, not the board's.** It has to be painted *before* the slot the
    // decoration is set into, and only the window knows where that slot is — a ground painted here would
    // be added to the painter after it and would wash the decoration out. See `QuillApp::paint_the_chrome`.
    let mut requests = Vec::new();
    // **No pump here.** Every open provider is pumped once a frame by `QuillApp::let_the_plugins_catch_up`,
    // which is what makes an agent's output arrive while its board is put away. Pumping again from the drawing
    // meant every terminal was read twice a frame, and reading one means rebuilding a screenful of its scrollback
    // and hashing it — so a board with three agents on it did six of those per frame, and the board drawn as both
    // a pane and a tab did more.
    let (page, chosen) = beside_the_rail(board, ui, look, area);
    let header = Rect::from_min_size(page.min, Vec2::new(page.width(), header_height(look)));
    requests.extend(view_switch(board, ui, look, header));
    let body = Rect::from_min_max(Pos2::new(page.min.x, header.max.y), page.max);
    // The lanes when the modal is open, because the modal **is** the ticket: drawing it here as well was one
    // ticket in two places, and pressing Start in one of them left the other showing a state it did not have.
    // The pane's own in-place detail is what a window too narrow for a modal falls back to.
    match board.detail().task.is_some() && !board.modal_open {
        true => requests.extend(detail::show(board, ui, look, body, false)),
        false => requests.extend(body_for(board, ui, look, body)),
    }
    take_the_chosen_view(board, chosen);
    requests
}

/// Draw the rail and answer the page beside it, and whichever view was pressed.
///
/// One function rather than two copies, because the pane and the tab lay the same two things out and a
/// board drawn one way and the same board drawn the other must not disagree about where its views are.
fn beside_the_rail(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
) -> (Rect, Option<View>) {
    let scale = look.scale();
    let width = RAIL * scale;
    // A pane too small for a rail and a lane draws no rail: a control that cannot apply is absent, and a
    // rail that took a fifth of a 300 point column would be a rail with nothing beside it. **Both
    // measurements**, because a pane can be short as well as narrow — a rail asked for more height than
    // there is would be a rectangle whose bottom is above its top, which draws nothing and answers clicks
    // at positions nobody can see. The views are still reachable, from the menu and from the command line.
    let tall = rail_height(look);
    let inset = rail_inset();
    if area.width() < width + inset + AFTER_RAIL * scale + 240.0 || area.height() < tall + PAD * 2.0 {
        return (area.shrink2(Vec2::new(PAD, 0.0)), None);
    }
    let rail_area =
        Rect::from_min_size(Pos2::new(area.min.x + inset, area.min.y + PAD), Vec2::new(width, tall));
    let chosen = rail(board, ui, look, rail_area);
    let page = Rect::from_min_max(
        Pos2::new(rail_area.max.x + AFTER_RAIL * scale, area.min.y),
        Pos2::new(area.max.x - PAD, area.max.y),
    );
    (page, chosen)
}

/// Act on a press in the rail, once the drawing that read the board is over.
fn take_the_chosen_view(board: &mut AgentTasks, chosen: Option<View>) {
    if let Some(view) = chosen {
        board.set_view(view);
        board.close_detail();
    }
}

/// The tab in the editing area: the lanes on the left and the open ticket on the right.
///
/// This is the arrangement the Markdown side by side view already uses, and it is what a whole editing
/// area is for. With no ticket open the lanes fill it.
pub fn tab(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    // **The ground is the window's, not the board's.** It has to be painted *before* the slot the
    // decoration is set into, and only the window knows where that slot is — a ground painted here would
    // be added to the painter after it and would wash the decoration out. See `QuillApp::paint_the_chrome`.
    let mut requests = Vec::new();
    // **No pump here.** Every open provider is pumped once a frame by `QuillApp::let_the_plugins_catch_up`,
    // which is what makes an agent's output arrive while its board is put away. Pumping again from the drawing
    // meant every terminal was read twice a frame, and reading one means rebuilding a screenful of its scrollback
    // and hashing it — so a board with three agents on it did six of those per frame, and the board drawn as both
    // a pane and a tab did more.
    let (page, chosen) = beside_the_rail(board, ui, look, area);
    let header = Rect::from_min_size(page.min, Vec2::new(page.width(), header_height(look)));
    requests.extend(view_switch(board, ui, look, header));
    let body = Rect::from_min_max(Pos2::new(page.min.x, header.max.y), page.max);
    let showing_a_ticket = board.detail().task.is_some() && !board.modal_open;
    if !showing_a_ticket || body.width() < 900.0 {
        requests.extend(match showing_a_ticket {
            true => detail::show(board, ui, look, body, false),
            false => body_for(board, ui, look, body),
        });
        take_the_chosen_view(board, chosen);
        return requests;
    }
    // Wide enough for both: the lanes take what they need and the ticket takes the rest, with a divider
    // drawn the way every divider in Quill is drawn.
    let split = (body.min.x + body.width() * 0.55).round();
    let lanes_area = Rect::from_min_max(body.min, Pos2::new(split, body.max.y));
    let detail_area = Rect::from_min_max(Pos2::new(split + 1.0, body.min.y), body.max);
    ui.painter().rect_filled(
        Rect::from_min_max(Pos2::new(split, body.min.y), Pos2::new(split + 1.0, body.max.y)),
        0,
        look.palette.divider,
    );
    requests.extend(body_for(board, ui, look, lanes_area));
    requests.extend(detail::show(board, ui, look, detail_area, true));
    take_the_chosen_view(board, chosen);
    requests
}

/// The Settings page.
pub fn settings(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    settings_page::show(board, ui, look)
}

/// Whichever of the four views is chosen.
fn body_for(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Request> {
    match board.current_view() {
        View::Board => lanes::show(board, ui, look, area),
        View::Backlog => lanes::listing(board, ui, look, area, View::Backlog),
        View::Completed => lanes::listing(board, ui, look, area, View::Completed),
        View::Epics => lanes::epics(board, ui, look, area),
    }
}

/// How wide the rail down the left of the board is, how round it is, and how big a button in it is.
///
/// Measured off `_agent_output/task-1765-vello-board/reference-board.png`: the rail there is 52 across
/// with a 26 point radius, eight points in from the edge of the pane, and the first lane starts 45 points
/// after it. That gap is not slack — it is the room a lane's own shadow needs, which reaches about 24
/// points, and without a rail there the first lane's shadow was cut off by the edge of the pane.
const RAIL: f32 = 52.0;
const RAIL_BUTTON: f32 = 36.0;
const RAIL_GAP: f32 = 10.0;
const RAIL_PAD: f32 = 12.0;
/// Between the rail and everything else.
const AFTER_RAIL: f32 = 24.0;
/// Between the rail and the edge of the pane.
///
/// **Asked for rather than written down**, because the number that matters is how far a raised surface's
/// shadow reaches and only `Lift` knows it: the canvas is cut to the pane, so a rail closer to the edge
/// than its own shadow has that shadow clipped and reads as a strip stuck to the side. It was written down
/// as 16 and the reach is 25.5, so it was still clipped — by a third of it — and the second review caught
/// the arithmetic where an eye had not.
fn rail_inset() -> f32 {
    crate::services::vello_canvas::Lift::Medium.reach().ceil()
}

/// How tall the rail is: as tall as the buttons in it, which is what the reference draws.
fn rail_height(look: &Look<'_>) -> f32 {
    // **Everything scales, or nothing does.** The height scaled only the buttons while the placement below
    // scaled the padding and the gaps as well, so above the default font size the last button hung out of
    // the bottom of the rail.
    let scale = look.scale();
    (RAIL_PAD * 2.0 + RAIL_BUTTON * View::ALL.len() as f32 + RAIL_GAP * (View::ALL.len() - 1) as f32)
        * scale
}

/// The rail down the left of the board: one icon button a view, the chosen one lit.
///
/// **This is the board's own rail and not Quill's.** `components::activity_bar` is the thin strip on the
/// window's left edge that puts panels away, and it belongs to the window: a plugin adding entries to it
/// would be a plugin deciding what the window's furniture is. The reference has a rail *inside* its own
/// page, which is a different thing — it is how the board switches between its four views — and that is
/// what this is.
fn rail(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Option<View> {
    let scale = look.scale();
    if look.chrome.is_recording() {
        look.chrome.raised(
            area,
            RAIL / 2.0 * scale,
            crate::services::vello_canvas::Fill::Solid(look.ground(look.palette.board_lane)),
            crate::services::vello_canvas::Lift::Medium,
        );
    } else {
        ui.painter().rect_filled(
            area,
            egui::CornerRadius::same((RAIL / 2.0 * scale) as u8),
            look.ground(look.palette.board_lane),
        );
    }
    let button = RAIL_BUTTON * scale;
    let mut chosen = None;
    for (index, view) in View::ALL.into_iter().enumerate() {
        let at = Rect::from_center_size(
            Pos2::new(
                area.center().x,
                area.min.y + RAIL_PAD * scale + button / 2.0 + index as f32 * (button + RAIL_GAP * scale),
            ),
            Vec2::splat(button),
        );
        let response =
            ui.interact(at, ui.id().with(("agent-tasks-view", view.label())), egui::Sense::click());
        let here = board.current_view() == view;
        let radius = 12.0 * scale;
        if look.chrome.is_recording() {
            if here {
                look.chrome.glow(at, radius, look.palette.board_accent.gamma_multiply(0.45), 8.0);
                look.chrome.rect(
                    at,
                    radius,
                    crate::services::vello_canvas::Fill::diagonal(
                        at,
                        lighten(look.palette.board_accent, 0.14),
                        darken(look.palette.board_accent, 0.22),
                    ),
                );
            }
            if response.hovered() && !here {
                ui.painter().rect_filled(
                    at,
                    egui::CornerRadius::same(radius as u8),
                    egui::Color32::from_white_alpha(16),
                );
            }
        } else if here {
            ui.painter().rect_filled(at, egui::CornerRadius::same(radius as u8), look.palette.board_accent);
        } else if response.hovered() {
            ui.painter().rect_filled(at, egui::CornerRadius::same(radius as u8), look.palette.control);
        }
        let tint = match here {
            true => look.palette.text_strong,
            false => look.palette.text_dim,
        };
        view_icon(view)(ui.painter(), at.center(), tint);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), here, view.label())
        });
        if response.clicked() {
            chosen = Some(view);
        }
    }
    chosen
}

/// The drawn mark for each view. Drawn rather than lettered, which is `design/style-guide.md`'s rule.
fn view_icon(view: View) -> fn(&egui::Painter, Pos2, egui::Color32) {
    match view {
        View::Board => crate::theme::icon::board,
        View::Backlog => crate::theme::icon::stack,
        View::Completed => crate::theme::icon::tick,
        View::Epics => crate::theme::icon::diamond,
    }
}

/// The strip across the top: the sprint's name, the search box and `+ Add Task`.
///
/// **One row.** It was two, because the four views were in it and one row could not hold all of that at a
/// pane's width — and the reference has one row, with its views down a rail instead. `task-1765` moved
/// them there, so what is left fits: a heading, a field that takes whatever is spare, and one button.
fn view_switch(
    board: &mut AgentTasks,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
) -> Vec<Request> {
    let mut requests = Vec::new();
    let painter = ui.painter().clone();
    let scale = look.scale();

    // The sprint's name and how many tickets are on the board, which is the explorer's footer's own idea.
    let title = match board.board().sprint.as_ref() {
        Some(sprint) => sprint.name.clone(),
        None => "No active sprint".to_owned(),
    };
    // Set in the bold face at 1.7 times the editor's size, which is the weight and the proportion the
    // picture gives it: `Current Sprint` is the one thing on the board meant to be read first.
    // `+ Add Task` at the end of the row, at the size the reference draws it: 127 by 44. Placed before
    // anything is drawn, because everything else is fitted into what it leaves.
    let add_size = Vec2::new(127.0, 44.0) * scale;
    let add = Rect::from_min_size(
        Pos2::new(area.max.x - add_size.x, area.center().y - add_size.y / 2.0),
        add_size,
    );
    // **One line, cut short with an ellipsis**, not wrapped. `Painter::layout` wraps, and a sprint with a
    // long name then took three lines out of a header one row tall and ran up over the lanes above it —
    // which is what giving way is *not*: a heading that is too long should get shorter, not taller.
    let mut job = egui::text::LayoutJob::single_section(
        title,
        egui::TextFormat {
            font_id: egui::FontId::new(
                look.font_size * 1.7,
                egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into()),
            ),
            color: look.palette.text_strong,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (add.min.x - 12.0 - area.min.x).max(1.0),
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    let heading = painter.layout_job(job);
    painter.galley(
        Pos2::new(area.min.x, area.center().y - heading.size().y / 2.0),
        heading.clone(),
        look.palette.text_strong,
    );
    let mut pen = area.min.x + heading.size().x + 10.0;
    let count = painter.layout_no_wrap(
        format!("\u{b7} {}", card::plural(board.board().total() as i64, "task")),
        egui::FontId::proportional(look.font_size - 1.0),
        look.palette.text_dim,
    );
    if pen + count.size().x < add.min.x - 12.0 {
        painter.galley(
            Pos2::new(pen, area.center().y - count.size().y / 2.0),
            count.clone(),
            look.palette.text_dim,
        );
        pen += count.size().x + 16.0;
    }
    // **Always drawn.** It is the one action in the header, and a control that vanished when the pane got
    // narrow was a board somebody could not add a ticket to — where what should give way is the heading,
    // which is a label. So the heading is clipped to the room left in front of it, and the button stays.
    let adding = primary_button(ui, look, add, "+ Add Task");
    // **The search box takes whatever is spare**, between the heading and the button, up to the width the
    // reference gives it. It was a fixed 200 points, which on a wide board left a broad empty band where
    // the reference has its search.
    let most = 460.0 * scale;
    let room = (add.min.x - 12.0) - pen;
    let width = room.min(most);
    let search = Rect::from_min_size(
        Pos2::new(add.min.x - 12.0 - width, area.center().y - add_size.y / 2.0),
        Vec2::new(width, add_size.y),
    );
    // 140 points unscaled, not 120 scaled: a field is wide enough to search in when it can hold a few
    // letters and its magnifier, and that does not get wider because the editor is set in 20 point text.
    // Scaled, a large font took the search box away on a window where it plainly fitted.
    let room_for_search = width > 140.0;
    if room_for_search && look.chrome.is_recording() {
        look.chrome.sunken(
            search,
            add_size.y / 2.0,
            look.ground(look.palette.board_well),
            crate::services::vello_canvas::Lift::Small,
        );
    }
    // With the decoration off, `search_field_over` draws its own frame — see the `ground` argument below.
    // The field the lanes already filter by. Everything behind it was built and tested and only the control
    // that sets it was missing — so the only way to search was the command line, and **an agent that
    // searched left the board filtered with nothing on screen saying so and no way to clear it.**
    let mut searched: Option<String> = None;
    if room_for_search {
        let mut query = board.query().to_owned();
        let response = crate::components::controls::search_field_over(
            ui,
            search,
            "Search tasks",
            "Search tasks",
            &mut query,
            !look.chrome.is_recording(),
        );
        if response.changed() {
            searched = Some(query);
        }
        // **Enter typed into this field is taken out of the frame.** egui's single-line editor gives up focus
        // on Enter and leaves the press in the input, so by the time the lanes are drawn no text box holds the
        // keyboard any more and the board took the same Enter as "open the ticket the ring is on". Somebody
        // searching for a ticket found a different one opened on top of the board.
        enter_was_used(ui, &response);
    }

    // What the board last said, under the heading, where there is always room for it.
    if !board.message().is_empty() {
        let said = painter.layout_no_wrap(
            board.message().to_owned(),
            egui::FontId::proportional(look.font_size - 1.5),
            look.palette.text_dim,
        );
        let at = Pos2::new(area.min.x, area.max.y - said.size().y);
        if at.x + said.size().x < search.min.x {
            painter.galley(at, said, look.palette.text_dim);
        }
    }

    if let Some(query) = searched {
        if let Err(problem) = board.search(&query) {
            requests.push(Request::Message(problem));
        }
    }
    if adding {
        if let Err(problem) = board.command_now("new-task", &[]).map(|_| ()) {
            requests.push(Request::Message(problem));
        }
    }
    requests
}

/// A colour moved towards white, and one moved towards black.
///
/// What a gradient's two ends are made of. The board never names a second colour for the light end of a
/// button: it lightens and darkens the one it already has, so the palette stays closed and a gradient can
/// never disagree with the flat colour it was derived from.
pub(crate) fn lighten(colour: egui::Color32, amount: f32) -> egui::Color32 {
    let mix = |channel: u8| {
        let value = f32::from(channel);
        (value + (255.0 - value) * amount.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgba_premultiplied(mix(colour.r()), mix(colour.g()), mix(colour.b()), colour.a())
}

pub(crate) fn darken(colour: egui::Color32, amount: f32) -> egui::Color32 {
    let mix = |channel: u8| (f32::from(channel) * (1.0 - amount.clamp(0.0, 1.0))).round() as u8;
    egui::Color32::from_rgba_premultiplied(mix(colour.r()), mix(colour.g()), mix(colour.b()), colour.a())
}

/// The one primary button on the board: `+ Add Task`.
///
/// A diagonal blue gradient with a blue glow under it and the word set bold in white, which is what the
/// picture shows and is a shape `epaint` has no answer to — its gradient brush is axis-aligned and its blur
/// is on a rectangle. With the decoration off it is the accent-filled button the board drew before.
pub(crate) fn primary_button(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: egui::Rect,
    label: &str,
) -> bool {
    let response = ui.interact(area, ui.id().with(("agent-tasks-primary", label)), egui::Sense::click());
    let ground = look.palette.board_accent;
    if look.chrome.is_recording() {
        look.chrome.glow(area, 14.0, ground.gamma_multiply(0.42), 9.0);
        look.chrome.raised(
            area,
            14.0,
            crate::services::vello_canvas::Fill::diagonal(area, lighten(ground, 0.14), darken(ground, 0.22)),
            crate::services::vello_canvas::Lift::Small,
        );
        if response.hovered() {
            ui.painter().rect_filled(area, egui::CornerRadius::same(14), egui::Color32::from_white_alpha(22));
        }
    } else {
        let flat = match response.hovered() {
            true => lighten(ground, 0.12),
            false => ground,
        };
        ui.painter().rect_filled(area, egui::CornerRadius::same(8), flat);
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(
            look.font_size - 3.0,
            egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into()),
        ),
        look.palette.text_strong,
    );
    ui.painter().galley(area.center() - galley.size() / 2.0, galley, look.palette.text_strong);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.clicked()
}

/// A field that names one of a short list and takes the next one when pressed.
///
/// The agent chooser under the New lane. It is drawn as the reference draws it — the name at the left of
/// its well and a chevron at the right — rather than as a centred word, because the shape of a control is
/// what says what it does: a centred word in a box reads as a label. Pressing it takes the next agent
/// rather than opening a list, which is what `components::controls::choice_button` already did and is
/// right for a choice between two: egui keeps one popup open at a time, and a dropdown here would shut
/// whatever else was open.
pub(crate) fn chooser_button(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: egui::Rect,
    label: &str,
    announced: &str,
) -> bool {
    let response =
        ui.interact(area, ui.id().with(("agent-tasks-chooser", announced)), egui::Sense::click());
    if look.chrome.is_recording() {
        if response.hovered() {
            ui.painter().rect_filled(
                area,
                egui::CornerRadius::same((area.height() / 2.0) as u8),
                egui::Color32::from_white_alpha(14),
            );
        }
    } else {
        ui.painter().rect(
            area,
            egui::CornerRadius::same((area.height() / 2.0) as u8),
            look.palette.field,
            egui::Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(look.font_size - 2.0),
        look.palette.text_control,
    );
    ui.painter().galley(
        Pos2::new(area.min.x + 14.0, area.center().y - galley.size().y / 2.0),
        galley,
        look.palette.text_control,
    );
    crate::theme::icon::chevron_down(
        ui.painter(),
        Pos2::new(area.max.x - 14.0, area.center().y),
        look.palette.text_dim,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), announced)
    });
    response.clicked()
}

/// A round button with an icon in it: the play button on a card, and the ones beside it.
///
/// A disc filled with a gradient along its own diagonal, lit from the same corner every shadow on the board
/// is, with a glow of its own colour under it — which is what the picture shows and what `epaint` has no
/// gradient to draw. With the decoration off it is the flat circle the board drew before.
pub(crate) fn round_button(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: egui::Rect,
    name: &str,
    draw: fn(&egui::Painter, egui::Pos2, egui::Color32),
) -> bool {
    let response = ui
        .interact(area, ui.id().with(("agent-tasks-round", name)), egui::Sense::click())
        .on_hover_text(name);
    let ground = look.palette.board_accent;
    let radius = area.width() / 2.0;
    if look.chrome.is_recording() {
        // Constant, and the hover is a wash on top: see `card::show` for why the decoration must not change
        // when the pointer moves.
        look.chrome.glow(area, radius, ground.gamma_multiply(0.45), 6.0);
        look.chrome.disc(
            area.center(),
            radius,
            crate::services::vello_canvas::Fill::diagonal(area, lighten(ground, 0.12), darken(ground, 0.18)),
        );
        if response.hovered() {
            ui.painter().circle_filled(area.center(), radius, egui::Color32::from_white_alpha(24));
        }
    } else {
        let flat = match response.hovered() {
            true => lighten(ground, 0.14),
            false => ground,
        };
        ui.painter().circle_filled(area.center(), radius, flat);
    }
    draw(ui.painter(), area.center(), look.palette.text_strong);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response.clicked()
}
/// A label drawn at a position, which is what nearly every line on the board is.
///
/// One function rather than the same four lines in twelve places, and it returns how wide it drew so a
/// caller can put the next thing after it.
pub(crate) fn text(
    painter: &egui::Painter,
    at: Pos2,
    said: &str,
    size: f32,
    tint: egui::Color32,
) -> f32 {
    let galley = painter.layout_no_wrap(said.to_owned(), egui::FontId::proportional(size), tint);
    let width = galley.size().x;
    painter.galley(at, galley, tint);
    width
}

/// Take Enter out of the frame when a single-line field has just acted on it.
///
/// egui's single-line editor gives up the keyboard when Enter is pressed and does not consume the press, so
/// anything drawn afterwards that listens for Enter sees it and believes no field was involved. The board's ring
/// opens a ticket on Enter and a modal's footer confirms on Enter, so both used to answer a key that was meant
/// for a field. Every field on the board that does something with Enter calls this after it.
pub(crate) fn enter_was_used(ui: &egui::Ui, response: &egui::Response) {
    if response.lost_focus() || response.has_focus() {
        ui.ctx().input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        });
    }
}

/// Whether a field was left by pressing Enter, taking that Enter out of the frame.
///
/// One function rather than the same two conditions written in four places, and it consumes as well as answers,
/// which is the half that was missing: see [`enter_was_used`].
pub(crate) fn enter_was_used_and_pressed(ui: &egui::Ui, response: &egui::Response) -> bool {
    let pressed = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if pressed {
        ui.ctx().input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        });
    }
    pressed
}

/// A label wrapped to `width` and cut off with an ellipsis after `lines` of it.
///
/// **It used to wrap without limit**, which is not what the name said and not what any caller wanted: a card
/// title of forty words drew down over the card's own footer and over the card under it, because the caller
/// advanced by one card height whatever was drawn. The same wrapping ran a long todo through the row beneath it
/// and a long scheduled command through the next schedule.
///
/// The two buttons that say whether a piece of markdown is read as its source or as markdown.
///
/// `task-28`. `at` is where the pair goes, right aligned, and the answer is the state the pair was left in, or
/// `None` when neither was pressed.
///
/// The icons are `icon::view_mode`'s own `Raw` and `Preview`, which are the pictures the editor's own view
/// buttons use for exactly this choice — so switching a description between source and markdown looks like
/// switching a `.md` file between them, because it is the same choice.
///
/// There is no check for a markdown plugin, because **there is no markdown plugin**: markdown is built into
/// `quill-core`. See `components::markdown_text`.
pub(crate) fn raw_or_rendered(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    at: Rect,
    id: &str,
    rendered: bool,
) -> Option<bool> {
    use crate::app::ViewMode;
    let mut chosen = None;
    let size = 18.0;
    for (index, (mode, wants)) in [(ViewMode::Raw, false), (ViewMode::Preview, true)].into_iter().enumerate() {
        let button = Rect::from_min_size(
            Pos2::new(at.max.x - size * 2.0 - 4.0 + index as f32 * (size + 4.0), at.min.y),
            Vec2::splat(size),
        );
        let on = rendered == wants;
        // Named for what it does rather than for what it draws, because that is what somebody reads out and what
        // a test asks for. Two controls in one window may not share a name, so the id of the thing being looked
        // at is part of it: a ticket shows one description and several comments at once.
        let name = match wants {
            true => format!("Read {id} as markdown"),
            false => format!("Read {id} as its source"),
        };
        let response = ui.interact(button, ui.id().with(("agent-tasks-view", id, index)), egui::Sense::click());
        let painter = ui.painter();
        if on {
            painter.rect_filled(button, egui::CornerRadius::same(look.corner_radius as u8), look.palette.selected_row);
        } else if response.hovered() {
            painter.rect_filled(button, egui::CornerRadius::same(look.corner_radius as u8), look.palette.control);
        }
        let tint = match on {
            true => look.palette.text_strong,
            false => look.palette.text_dim,
        };
        crate::theme::icon::view_mode(painter, button.shrink(4.0), mode, tint);
        let said = name.clone();
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &said));
        let _ = response.clone().on_hover_text(name);
        if response.clicked() && !on {
            chosen = Some(wants);
        }
    }
    chosen
}

/// One value chosen from a list, drawn into `at`, answering what was chosen when it changed.
///
/// `task-28`: "Dropdowns. We need UI dropdowns for values." This is the **only** place the plugin draws a list
/// of values, so a ticket's `Model` and the Settings page's `Default model` are the same control offering the
/// same list — they used to be a text field and a text field, and each would have had to be corrected
/// separately.
///
/// `components::controls::dropdown` is what it is made of, which the toolbar and `Settings -> Appearance`
/// already use, so it opens, closes and reads like every other dropdown in Quill. `options` is `(value, said)`
/// pairs: the value written down and the words a person reads, because `agent_done` is not what anybody calls
/// that lane. `empty` is what the list calls holding nothing, for a value that may hold nothing; `None` means
/// the value is required and there is no way to clear it.
///
/// Choosing what is already chosen answers `None`, so nothing is written and nothing is reported.
pub(crate) fn value_dropdown(
    ui: &mut egui::Ui,
    at: Rect,
    name: &str,
    options: &[(String, String)],
    chosen: &str,
    empty: Option<&str>,
) -> Option<String> {
    let showing = options
        .iter()
        .find(|(value, _)| value == chosen)
        .map(|(_, said)| said.clone())
        .unwrap_or_else(|| empty.unwrap_or("").to_owned());
    let rows: Vec<(String, String)> = match empty {
        Some(said) => std::iter::once((String::new(), said.to_owned())).chain(options.iter().cloned()).collect(),
        None => options.to_vec(),
    };
    let picked = crate::components::controls::dropdown(ui, at, &showing, name, None, |ui| {
        let mut picked = None;
        for (value, said) in &rows {
            // `selectable_label` rather than a painted row, because the list is inside egui's own popup and this
            // is what every other dropdown in Quill puts in one.
            if ui.selectable_label(value == chosen, said).clicked() {
                picked = Some(value.clone());
            }
        }
        picked
    });
    picked.filter(|value| value != chosen)
}

/// The text is shortened until it fits rather than the drawing being clipped, so that what is cut off is
/// announced by an ellipsis instead of being silently invisible. A first slice keeps the search cheap on a
/// description sized string: how many characters could possibly fit is known from the width and the size, and
/// there is no point laying out the other ten thousand.
pub(crate) fn clipped(
    painter: &egui::Painter,
    at: Pos2,
    said: &str,
    size: f32,
    tint: egui::Color32,
    width: f32,
    lines: usize,
) {
    clipped_in(painter, at, said, egui::FontId::proportional(size), tint, width, lines);
}

/// The same, in a face the caller names.
///
/// A card's title is set in the bold face, which is what the picture shows and is the one place on the board
/// where the weight is doing work: it is the line a person reads to find the ticket they want.
pub(crate) fn clipped_in(
    painter: &egui::Painter,
    at: Pos2,
    said: &str,
    font: egui::FontId,
    tint: egui::Color32,
    width: f32,
    lines: usize,
) {
    let size = font.size;
    let most = (width / (size * 0.4)).max(1.0) as usize * lines.max(1) + 8;
    let short = match said.char_indices().nth(most) {
        Some((at, _)) => &said[..at],
        None => said,
    };
    let mut galley = painter.layout(short.to_owned(), font.clone(), tint, width);
    let room = size * 1.35 * lines.max(1) as f32;
    if galley.size().y > room {
        // Trimmed a character at a time from the end, which is at most a handful of steps from the slice above.
        let mut fits = short;
        while !fits.is_empty() {
            let cut = match fits.char_indices().next_back() {
                Some((at, _)) => at,
                None => break,
            };
            fits = &said[..cut];
            let tried = painter.layout(format!("{}…", fits.trim_end()), font.clone(), tint, width);
            if tried.size().y <= room {
                galley = tried;
                break;
            }
        }
    }
    painter.galley(at, galley, tint);
}
