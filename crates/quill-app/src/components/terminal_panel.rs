//! The terminal along the bottom of the window: the tabs, the grid, and the keyboard and mouse into it.
//!
//! The terminal itself is in `quill-terminal`, which has no user interface dependencies. This draws what
//! that crate reports and sends it what the keyboard and the mouse did. Nothing here parses an escape
//! sequence and nothing there knows what egui is.
//!
//! How the grid is drawn, and why in that order: one rectangle for each run of cells that share a
//! background colour, then the selection over them, then one glyph a cell out of Quill's own atlas, then the
//! underline and strikethrough rules, then the cursor last so nothing covers it. It is the same glyph atlas
//! the editor draws from, at the monospaced family, so the terminal is set in the same ink as the document.
//!
//! The size of the grid is worked out here, from the tile's size and the cell size, and told to the session,
//! which tells both the emulator and the program on the far side. Getting that wrong is what leaves a full
//! screen program drawing in the wrong place, so it happens in one place only.

use egui::{Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use quill_terminal::keys::{self, KeyPress, Modifiers as TermModifiers};
use quill_terminal::mouse;
use quill_terminal::screen::{CursorShape, Screen};
use quill_terminal::session::{SelectionKind, SessionSettings, Size};
use quill_terminal::Tabs;

use crate::components::controls;
use crate::components::file_tabs;
use crate::components::splitter;
use crate::services::text_renderer::TextRenderer;
use crate::theme::{color, icon};

/// How tall the strip holding the tabs is.
pub const HEADER: f32 = 32.0;
/// Space between the grid and the edge of the tile.
///
/// `pub(crate)` because the run tile is the terminal tile's sibling and is drawn from the same
/// measurements — `task-1683` §6.1. Two tiles that almost agreed about their padding would be two
/// grids that did not line up when the bottom of the window was switched between them.
pub(crate) const PADDING_X: f32 = 10.0;
pub(crate) const PADDING_Y: f32 = 6.0;

/// The terminal tile's own state.
pub struct TerminalPanel {
    /// False when the tile is put away, which is what `View -> Terminal` switches.
    pub visible: bool,
    /// True when the keyboard is talking to the terminal rather than to the document.
    pub focused: bool,
    pub tabs: Tabs,
    /// True while the mouse is dragging out a selection.
    selecting: bool,
    /// The rectangle the grid last filled, for the tests.
    grid_area: Rect,
}

impl TerminalPanel {
    pub fn new(working_directory: Option<std::path::PathBuf>) -> Self {
        Self {
            visible: false,
            focused: false,
            tabs: Tabs::new(SessionSettings { working_directory, ..SessionSettings::default() }),
            selecting: false,
            grid_area: Rect::ZERO,
        }
    }

    /// Where the grid was last drawn, which a test uses to work out where a cell is on screen.
    pub fn grid_area(&self) -> Rect {
        self.grid_area
    }
}

/// What the tile asks the window to do.
#[derive(Debug, Default)]
pub struct PanelOutcome {
    /// The tile was put away.
    pub hide: bool,
    /// Another tab was asked for.
    pub new_tab: bool,
    /// The tile was clicked, so it should have the keyboard.
    pub take_focus: bool,
    /// Text to put on the clipboard, from a copy or from a program asking.
    pub copy: Option<String>,
    /// A tab was right clicked: which one, and where the pointer was.
    pub menu: Option<(usize, Pos2)>,
    /// The tile is being carried to another edge of the window, or its header was right clicked.
    ///
    /// The divider that resizes it is drawn by the window now rather than here: since `task-1697`
    /// the tile is not always along the bottom, so its inner edge is not always its top.
    pub grab: crate::components::dock::Grab,
}

/// Draw the tile into `area` and take its input.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut TerminalPanel,
    renderer: &TextRenderer,
    font_size: f32,
    opacity: f32,
) -> PanelOutcome {
    let mut outcome = PanelOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::toolbar(), opacity));

    let header = Rect::from_min_size(
        Pos2::new(area.left(), area.top() + 1.0),
        Vec2::new(area.width(), HEADER),
    );
    show_header(ui, header, panel, &mut outcome);
    splitter::line(
        &ui.painter_at(area),
        Pos2::new(area.left(), header.bottom()),
        Pos2::new(area.right(), header.bottom()),
    );

    let grid = Rect::from_min_max(Pos2::new(area.left(), header.bottom() + 1.0), area.max);
    panel.grid_area = grid;
    show_grid(ui, grid, panel, renderer, font_size, opacity, &mut outcome);

    // Anything the program asked to be put on the clipboard, which OSC 52 is how a program does.
    if let Some(session) = panel.tabs.active_mut() {
        if let Some(text) = session.take_clipboard() {
            outcome.copy = Some(text);
        }
    }
    outcome
}

/// The strip along the top: the word `Terminal`, the tabs, a button that adds one, and one that puts the
/// tile away.
fn show_header(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut TerminalPanel,
    outcome: &mut PanelOutcome,
) {
    // The handle first, over the whole strip, so the tabs and the buttons added after it take the
    // points they cover and this is left with the heading and the empty space beside it. See
    // `components::dock` for why it has to be this way round.
    outcome.grab = crate::components::dock::handle(ui, area, crate::app::dock::Panel::Terminal);
    let painter = ui.painter_at(area);
    let heading = painter.layout_no_wrap(
        "Terminal".to_owned(),
        egui::FontId::proportional(12.0),
        color::text_dim(),
    );
    painter.galley(
        Pos2::new(area.left() + 16.0, area.center().y - heading.size().y / 2.0),
        heading.clone(),
        color::text_dim(),
    );

    let after = tab_strip(ui, area, area.left() + 16.0 + heading.size().x + 18.0, panel, outcome);

    let add = Rect::from_center_size(Pos2::new(after + 11.0, area.center().y), Vec2::splat(22.0));
    if controls::icon_button(ui, add, "New terminal tab", icon::plus) {
        outcome.new_tab = true;
    }

    let hide = Rect::from_center_size(
        Pos2::new(area.right() - 22.0, area.center().y),
        Vec2::splat(22.0),
    );
    if controls::icon_button(ui, hide, "Hide the terminal", icon::collapse) {
        outcome.hide = true;
    }
}

/// The tabs themselves, left to right from `pen`. Returns the x the strip ended at, which is where the
/// plus goes.
///
/// A drag is settled here rather than by the window, which is where a file tab's drag has to be settled:
/// there is **one** strip of terminal tabs, so the strip a tab is picked up from is the strip it is
/// dropped on and nothing outside this function could know better where it landed. It goes through
/// [`quill_terminal::Tabs::move_tab`], which is what `quill-cli terminal move` calls too, so a
/// rearrangement made with the pointer and one made from a script are the same rearrangement.
fn tab_strip(
    ui: &mut egui::Ui,
    area: Rect,
    pen: f32,
    panel: &mut TerminalPanel,
    outcome: &mut PanelOutcome,
) -> f32 {
    let names = panel.tabs.names();
    let active = panel.tabs.active_index();
    let mut hit = TabHit::default();
    let mut strip = file_tabs::Strip { area, tabs: Vec::new() };
    let mut pen = pen;
    for (index, name) in names.iter().enumerate() {
        let rect = draw_tab(ui, area, pen, name, index == active, index, &mut hit);
        strip.tabs.push(rect);
        pen = rect.right() + 6.0;
    }

    // Where the tab being carried would land, worked out once every tab has said where it is. The mark
    // is the one the file tabs draw, from the same two functions, so a terminal tab and a file tab
    // follow the pointer in the same way.
    if let Some((index, pointer)) = hit.dragging {
        let position = strip.position_at(pointer.x);
        file_tabs::insertion_mark(&ui.painter_at(area), &strip, position);
        if hit.dropped {
            panel.tabs.move_tab(index, position);
            outcome.take_focus = true;
        }
    }
    if let Some(index) = hit.close {
        panel.tabs.close(index);
        if panel.tabs.is_empty() {
            outcome.hide = true;
        }
    } else if let Some(index) = hit.show {
        panel.tabs.show(index);
        outcome.take_focus = true;
    }
    if let Some((index, at)) = hit.menu {
        // The tab is shown first, so every entry in its menu is about "the terminal tab that is
        // showing" and needs no argument. A file tab's menu is opened the same way.
        panel.tabs.show(index);
        outcome.menu = Some((index, at));
    }
    pen
}

/// What the pointer did to the tabs this frame. One value rather than five, because they are settled
/// together once the whole strip has been drawn and the drag needs every tab's rectangle.
#[derive(Default)]
struct TabHit {
    show: Option<usize>,
    close: Option<usize>,
    menu: Option<(usize, Pos2)>,
    /// Which tab is being carried, and where the pointer is now.
    dragging: Option<(usize, Pos2)>,
    /// The drag ended on this frame.
    dropped: bool,
}

/// One tab: the pill, its name and its close cross. Returns the rectangle it filled.
fn draw_tab(
    ui: &mut egui::Ui,
    area: Rect,
    pen: f32,
    name: &str,
    active: bool,
    index: usize,
    hit: &mut TabHit,
) -> Rect {
    let painter = ui.painter_at(area);
    let label = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        if active { color::text_strong() } else { color::text_control() },
    );
    let tab = Rect::from_min_size(
        Pos2::new(pen, area.center().y - 11.0),
        Vec2::new(label.size().x + 38.0, 22.0),
    );
    // A tab senses a drag as well as a click, which is how it is rearranged. egui only calls a press a
    // drag once the pointer has moved far enough, so a click is still a click.
    let response = ui
        .interact(tab, ui.id().with(("terminal-tab", index)), Sense::click_and_drag())
        .on_hover_text(format!("Terminal tab: {name}"));
    if response.dragged() || response.drag_stopped() {
        if let Some(pointer) = response.interact_pointer_pos() {
            hit.dragging = Some((index, pointer));
            hit.dropped = response.drag_stopped();
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if active {
        painter.rect(
            tab,
            CornerRadius::same(4),
            color::selected_row(),
            Stroke::new(1.0, color::accent().gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(tab, CornerRadius::same(4), color::control());
    }
    // A tab being carried is outlined, so it is clear which one is in the air.
    if response.dragged() {
        painter.rect(
            tab,
            CornerRadius::same(4),
            color::control(),
            Stroke::new(1.0, color::accent()),
            egui::StrokeKind::Inside,
        );
    }
    painter.galley(
        Pos2::new(tab.left() + 10.0, tab.center().y - label.size().y / 2.0),
        label,
        color::text_control(),
    );
    let shut = Rect::from_center_size(
        Pos2::new(tab.right() - 12.0, tab.center().y),
        Vec2::splat(16.0),
    );
    let shut_response = ui
        .interact(shut, ui.id().with(("terminal-close", index)), Sense::click())
        .on_hover_text(format!("Close {name}"));
    icon::cross(&painter, shut.center(), color::text_dim());
    shut_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {name}"))
    });
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            active,
            format!("Terminal tab: {name}"),
        )
    });
    if shut_response.clicked() {
        hit.close = Some(index);
    } else if response.clicked() {
        hit.show = Some(index);
    }
    if response.middle_clicked() {
        hit.close = Some(index);
    }
    // A right click opens the tab's own menu, which is where `Rename...` is.
    if response.secondary_clicked() {
        if let Some(at) = response.interact_pointer_pos().or_else(|| response.hover_pos()) {
            hit.menu = Some((index, at));
        }
    }
    tab
}

/// The terminal tile's grid: the tab that is showing, or the reason there is not one.
fn show_grid(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut TerminalPanel,
    renderer: &TextRenderer,
    font_size: f32,
    opacity: f32,
    outcome: &mut PanelOutcome,
) {
    // No tab open: say so rather than leaving an empty tile, and say why if a shell would not start.
    let empty = panel
        .tabs
        .last_error
        .clone()
        .unwrap_or_else(|| "No terminal. Press the plus to open one.".to_owned());
    let focused = panel.focused;
    let mut selecting = panel.selecting;
    let grid_outcome = grid(
        ui,
        area,
        panel.tabs.active_mut(),
        &mut selecting,
        focused,
        "terminal-grid",
        &empty,
        renderer,
        font_size,
        opacity,
    );
    panel.selecting = selecting;
    outcome.take_focus |= grid_outcome.take_focus;
    if let Some(text) = grid_outcome.copy {
        outcome.copy = Some(text);
    }
}

/// What a grid produced this frame.
///
/// Two things, because they are the only two a grid decides: everything else it does is to the
/// session it was handed.
#[derive(Debug, Default)]
pub(crate) struct GridOutcome {
    /// The grid was clicked, so whichever tile it belongs to should have the keyboard.
    pub take_focus: bool,
    /// Text to put on the clipboard, from a copy or from a program asking.
    pub copy: Option<String>,
}

/// One session's grid: work out its size, tell the session, draw what it reports, and take the
/// input.
///
/// This is the whole of what the terminal tile and the run tile share, and it is shared rather than
/// copied because a run **is** a terminal as far as the person watching it is concerned: the same
/// emulator, the same colours, the same selection, the same clipboard rules, and keyboard into the
/// program because `node` asking a question deserves an answer. `task-1683` §6.1.
///
/// `id` keeps the two tiles' widgets apart — egui identifies a widget by its id, and two grids
/// sharing one would be one widget drawn twice. `empty` is what to say when there is no session,
/// which is the one thing the two tiles really do differ about.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grid(
    ui: &mut egui::Ui,
    area: Rect,
    session: Option<&mut quill_terminal::Session>,
    selecting: &mut bool,
    focused: bool,
    id: &str,
    empty: &str,
    renderer: &TextRenderer,
    font_size: f32,
    opacity: f32,
) -> GridOutcome {
    let mut outcome = GridOutcome::default();
    let cell = renderer.cell_metrics(font_size);
    let columns = ((area.width() - PADDING_X * 2.0) / cell.width).floor().max(1.0) as usize;
    let rows = ((area.height() - PADDING_Y * 2.0) / cell.height).floor().max(1.0) as usize;
    let wanted = Size::new(rows, columns).with_cell(cell.width, cell.height);

    // Everything below the tabs takes clicks, so a click anywhere in the grid moves the keyboard to the
    // tile.
    let response = ui.interact(area, ui.id().with(id), Sense::click_and_drag());

    let Some(session) = session else {
        let painter = ui.painter_at(area);
        let galley =
            painter.layout_no_wrap(empty.to_owned(), egui::FontId::proportional(12.0), color::text_faint());
        painter.galley(
            Pos2::new(area.left() + PADDING_X + 4.0, area.top() + PADDING_Y + 6.0),
            galley,
            color::text_faint(),
        );
        return outcome;
    };

    // **Pumped before it is resized**, which is the order that matters rather than a tidiness:
    // `Session::resize` refuses to tell a program that has ended, because telling it wipes what it
    // wrote — and whether it has ended is only known once the events it sent have been read. The
    // other way round, a program that exited since the last frame is still thought to be running
    // for one more resize, and that resize is the one that empties the tab. Measured on
    // `task-1683`: `cmd /c echo something` lost its output every single time.
    session.pump();
    session.resize(wanted);
    let screen = session.snapshot();

    // The whole tile is filled first, so the strip narrower than one cell at the right and the bottom is the
    // terminal's own background rather than whatever was there before the tile was dragged smaller.
    let background = crate::theme::faded(
        Color32::from_rgb(screen.background.r, screen.background.g, screen.background.b),
        opacity,
    );
    ui.painter_at(area).rect_filled(area, CornerRadius::ZERO, background);

    let origin = Pos2::new(area.left() + PADDING_X, area.top() + PADDING_Y);
    let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
    painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
    paint(&painter_ui, renderer, &screen, origin, cell, font_size, focused, opacity);

    // Input, once the drawing is decided, so that a key press is acted on with the size the program already
    // knows about.
    handle_input(ui, session, selecting, focused, &response, &screen, origin, cell, &mut outcome);

    // Anything the program asked to be put on the clipboard, which OSC 52 is how a program does.
    if let Some(text) = session.take_clipboard() {
        outcome.copy = Some(text);
    }
    outcome
}

/// Draw one screen.
fn paint(
    ui: &egui::Ui,
    renderer: &TextRenderer,
    screen: &Screen,
    origin: Pos2,
    cell: crate::services::text_renderer::CellMetrics,
    font_size: f32,
    focused: bool,
    opacity: f32,
) {
    let painter = ui.painter();
    let at = |row: usize, column: usize| {
        Pos2::new(origin.x + column as f32 * cell.width, origin.y + row as f32 * cell.height)
    };

    // The backgrounds first, one rectangle for each run of cells that share a colour, so a line with a
    // coloured background is one shape rather than eighty.
    let terminal_background = screen.background;
    for row in 0..screen.rows {
        let mut column = 0;
        while column < screen.columns {
            let Some(start) = screen.cell(row, column) else {
                break;
            };
            let colour = start.background;
            let mut end = column + 1;
            while end < screen.columns
                && screen.cell(row, end).is_some_and(|cell| cell.background == colour)
            {
                end += 1;
            }
            if colour != terminal_background {
                let rect = Rect::from_min_max(
                    at(row, column),
                    Pos2::new(at(row, end).x, at(row, column).y + cell.height),
                );
                painter.rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    crate::theme::faded(Color32::from_rgb(colour.r, colour.g, colour.b), opacity),
                );
            }
            column = end;
        }
    }

    // The selection, over the backgrounds and under the text, as it is in the editor.
    if let Some(range) = &screen.selection {
        for index in range.clone() {
            let row = index / screen.columns;
            let column = index % screen.columns;
            if row >= screen.rows {
                break;
            }
            let rect = Rect::from_min_size(at(row, column), Vec2::new(cell.width, cell.height));
            painter.rect_filled(rect, CornerRadius::ZERO, color::text_selection());
        }
    }

    // The glyphs. Collected before the atlas texture is uploaded, and the whole pass repeated if collecting
    // filled the atlas and cleared it, for the reason `editor_view::paint_text` records.
    let mut placed: Vec<(Rect, egui::Rect, Color32)> = Vec::new();
    for _ in 0..3 {
        let generation = renderer.generation();
        placed.clear();
        for row in 0..screen.rows {
            for column in 0..screen.columns {
                let Some(cell_data) = screen.cell(row, column) else {
                    continue;
                };
                if cell_data.is_blank() {
                    continue;
                }
                let style = renderer.terminal_style(font_size, cell_data.bold, cell_data.italic);
                let colour = Color32::from_rgb(
                    cell_data.foreground.r,
                    cell_data.foreground.g,
                    cell_data.foreground.b,
                );
                let pen = at(row, column);
                for character in
                    std::iter::once(cell_data.character).chain(cell_data.marks.iter().copied())
                {
                    let Some(glyph) = renderer.glyph(character, &style) else {
                        continue;
                    };
                    // Snapped to whole pixels: a glyph drawn on a fraction of a pixel is resampled, which
                    // softens every letter in the grid.
                    let position = Pos2::new(
                        (pen.x + glyph.offset.x).round(),
                        (pen.y + cell.ascent + glyph.offset.y).round(),
                    );
                    placed.push((Rect::from_min_size(position, glyph.size), glyph.uv, colour));
                }
            }
        }
        if renderer.generation() == generation {
            break;
        }
    }
    let texture = renderer.texture(ui.ctx());
    let mut mesh = Mesh::with_texture(texture);
    for (rect, uv, colour) in placed {
        mesh.add_rect_with_uv(rect, uv, colour);
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }

    // Underline and strikethrough, drawn as rules from the cell's own measurements rather than baked into
    // the glyphs, which is how the editor draws them too.
    for row in 0..screen.rows {
        for column in 0..screen.columns {
            let Some(cell_data) = screen.cell(row, column) else {
                continue;
            };
            if !cell_data.underline && !cell_data.strikethrough {
                continue;
            }
            let colour = Color32::from_rgb(
                cell_data.foreground.r,
                cell_data.foreground.g,
                cell_data.foreground.b,
            );
            let pen = at(row, column);
            if cell_data.underline {
                let y = (pen.y + cell.ascent + 2.0).round();
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(pen.x, y), Vec2::new(cell.width, 1.0)),
                    CornerRadius::ZERO,
                    colour,
                );
            }
            if cell_data.strikethrough {
                let y = (pen.y + cell.ascent * 0.65).round();
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(pen.x, y), Vec2::new(cell.width, 1.0)),
                    CornerRadius::ZERO,
                    colour,
                );
            }
        }
    }

    // The cursor last, so nothing covers it. Solid when the terminal has the keyboard and an outline when it
    // does not, which is what says where the keys are going.
    if let Some(cursor) = screen.cursor {
        let pen = at(cursor.row, cursor.column);
        let full = Rect::from_min_size(pen, Vec2::new(cell.width, cell.height));
        let shape = match cursor.shape {
            CursorShape::Block => full,
            CursorShape::Beam => Rect::from_min_size(pen, Vec2::new(2.0, cell.height)),
            CursorShape::Underline => Rect::from_min_size(
                Pos2::new(pen.x, pen.y + cell.height - 2.0),
                Vec2::new(cell.width, 2.0),
            ),
        };
        if focused {
            painter.rect_filled(shape, CornerRadius::ZERO, color::accent());
            // The character under a solid block is drawn again in the background colour, so it can still be
            // read through the cursor.
            if let Some(under) = screen.cell(cursor.row, cursor.column) {
                if cursor.shape == CursorShape::Block && !under.is_blank() {
                    let style = renderer.terminal_style(font_size, under.bold, under.italic);
                    if let Some(glyph) = renderer.glyph(under.character, &style) {
                        let position = Pos2::new(
                            (pen.x + glyph.offset.x).round(),
                            (pen.y + cell.ascent + glyph.offset.y).round(),
                        );
                        let mut mesh = Mesh::with_texture(renderer.texture(ui.ctx()));
                        mesh.add_rect_with_uv(
                            Rect::from_min_size(position, glyph.size),
                            glyph.uv,
                            Color32::from_rgb(
                                screen.background.r,
                                screen.background.g,
                                screen.background.b,
                            ),
                        );
                        painter.add(Shape::mesh(mesh));
                    }
                }
            }
        } else {
            painter.rect_stroke(
                full,
                CornerRadius::ZERO,
                Stroke::new(1.0, color::accent()),
                egui::StrokeKind::Inside,
            );
        }
    }

    // How far back the view is scrolled, said in words in the corner, because a terminal with no scroll bar
    // otherwise gives no sign that there is more above.
    if screen.scrollback > 0 {
        let text = format!("{} lines back", screen.scrollback);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color::text_dim());
        let at = Pos2::new(
            ui.max_rect().right() - 12.0 - galley.size().x,
            ui.max_rect().top() + 6.0,
        );
        painter.rect_filled(
            Rect::from_min_size(at, galley.size()).expand(4.0),
            CornerRadius::same(4),
            color::menu(),
        );
        painter.galley(at, galley, color::text_dim());
    }
}

/// The keyboard and the mouse, into one session.
#[allow(clippy::too_many_arguments)]
fn handle_input(
    ui: &mut egui::Ui,
    session: &mut quill_terminal::Session,
    selecting: &mut bool,
    focused: bool,
    response: &egui::Response,
    screen: &Screen,
    origin: Pos2,
    cell: crate::services::text_renderer::CellMetrics,
    outcome: &mut GridOutcome,
) {
    let modifiers = ui.input(|input| input.modifiers);
    let cell_at = |position: Pos2| -> (usize, usize) {
        let column = ((position.x - origin.x) / cell.width).floor().max(0.0) as usize;
        let row = ((position.y - origin.y) / cell.height).floor().max(0.0) as usize;
        (row.min(screen.rows.saturating_sub(1)), column.min(screen.columns.saturating_sub(1)))
    };

    // The mouse. A program that asked to be told about clicks is told; otherwise a drag selects text, and
    // holding shift always selects, which is the only way to copy out of such a program.
    let mouse_mode = session.mouse_mode();
    let report_to_program = mouse_mode.reports_clicks() && !modifiers.shift;

    if response.clicked() || response.drag_started() {
        outcome.take_focus = true;
    }
    if let Some(position) = response.interact_pointer_pos() {
        let (row, column) = cell_at(position);
        let terminal_modifiers = mouse::Modifiers {
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.ctrl && !modifiers.mac_cmd,
        };
        if report_to_program {
            let kind = if response.drag_started() || response.clicked() {
                mouse::Kind::Press
            } else if response.dragged() {
                mouse::Kind::Drag
            } else {
                mouse::Kind::Release
            };
            if let Some(bytes) =
                mouse::report(mouse_mode, kind, mouse::Button::Left, row, column, terminal_modifiers)
            {
                session.send(bytes);
            }
            if response.drag_stopped() || response.clicked() {
                if let Some(bytes) = mouse::report(
                    mouse_mode,
                    mouse::Kind::Release,
                    mouse::Button::Left,
                    row,
                    column,
                    terminal_modifiers,
                ) {
                    session.send(bytes);
                }
            }
        } else if response.double_clicked() {
            session.begin_selection(row, column, SelectionKind::Word);
            *selecting = false;
        } else if response.drag_started() || (response.clicked() && !*selecting) {
            session.begin_selection(row, column, SelectionKind::Simple);
            *selecting = true;
        } else if response.dragged() {
            session.extend_selection(row, column);
        }
        if !report_to_program && response.drag_stopped() {
            *selecting = false;
        }
    }

    // The wheel. On the ordinary screen it moves the view through the history. On a program's own screen
    // there is no history, so it is sent as arrow keys when the program asked for that, and as a wheel
    // report when it asked about the mouse.
    let wheel = ui.input(|input| input.smooth_scroll_delta.y);
    if wheel.abs() > 0.5 && response.hovered() {
        let lines = (wheel / cell.height).round() as i32;
        let lines = if lines == 0 { wheel.signum() as i32 } else { lines };
        if mouse_mode.reports_clicks() && !modifiers.shift {
            let button = if lines > 0 { mouse::Button::WheelUp } else { mouse::Button::WheelDown };
            let (row, column) = response.hover_pos().map(cell_at).unwrap_or((0, 0));
            for _ in 0..lines.abs().min(5) {
                if let Some(bytes) = mouse::report(
                    mouse_mode,
                    mouse::Kind::Press,
                    button,
                    row,
                    column,
                    mouse::Modifiers::default(),
                ) {
                    session.send(bytes);
                }
            }
        } else if session.alternate_scroll() {
            let key = if lines > 0 { keys::Key::Up } else { keys::Key::Down };
            let mode = session.mode();
            for _ in 0..lines.abs().min(5) {
                if let Some(bytes) = keys::encode(KeyPress::plain(key), mode) {
                    session.send(bytes);
                }
            }
        } else {
            session.scroll(lines);
        }
    }

    if !focused {
        return;
    }
    // A box that takes typing, such as the explorer's filter, has been clicked into. It keeps the keyboard
    // until it is clicked away from, so the terminal stands aside: taking the keys here would leave the
    // filter box impossible to type in while the terminal was open. The editing area asks the same
    // question in the same words, so there is one answer to it rather than two that could drift.
    if crate::app::text_box_has_the_keyboard(ui.ctx()) {
        return;
    }
    // And a modal, which mostly has no field to answer the question above. The editing area stands
    // aside in the same words for the same reason.
    //
    // **Another** modal, not this grid's own. A grid drawn inside a dialog — the Agent-Tasks ticket modal's
    // terminal — is the thing the keys are for, and asking whether any modal was open said no to it because its
    // own was. The layer is what tells the two apart.
    if crate::app::another_modal_has_the_keyboard(ui.ctx(), ui.layer_id()) {
        return;
    }

    // The keyboard. egui's own focus is cleared while the terminal has it, so that Tab and Escape are sent
    // to the program instead of moving between the window's controls.
    ui.memory_mut(|memory| memory.stop_text_input());

    let events = ui.input(|input| input.events.clone());
    // The modifiers held this frame. `Event::Copy` and `Event::Cut` carry none of their own — egui
    // consumed the key press that made them — so this is the only way to tell `Ctrl+C` from
    // `Ctrl+Shift+C`, and the control key from the Apple one.
    let held = ui.input(|input| input.modifiers);
    let mode = session.mode();
    let mut to_send: Vec<Vec<u8>> = Vec::new();
    let mut copy = None;
    let mut clear_selection = false;
    for event in &events {
        match event {
            egui::Event::Copy | egui::Event::Cut => {
                let cut = matches!(event, egui::Event::Cut);
                match clipboard_key(cut, session.selected_text(), &held, mode) {
                    Clipboard::Copy(text) => {
                        copy = Some(text);
                        clear_selection = true;
                    }
                    Clipboard::Send(bytes) => to_send.push(bytes),
                    Clipboard::Nothing => {}
                }
            }
            egui::Event::Paste(text) => {
                to_send.push(keys::paste(text, mode));
            }
            // The text egui reports, rather than the letter worked out from the key, so that a keyboard
            // layout, a dead key and an input method all reach the program as what was typed.
            egui::Event::Text(text) if !text.chars().any(char::is_control) => {
                to_send.push(text.clone().into_bytes());
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                if let Some(press) = key_press(*key, modifiers) {
                    // A key press with no modifier that egui also reported as text is left to the text
                    // event, so a letter is not sent twice.
                    if let Some(bytes) = keys::encode(press, mode) {
                        to_send.push(bytes);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(text) = copy {
        outcome.copy = Some(text);
    }
    // The selection goes as soon as it has been copied. Left behind, it would swallow the next
    // Ctrl+C as well, and the one after that, so dragging the mouse once would make a program
    // impossible to stop.
    if clear_selection {
        session.clear_selection();
    }
    if !to_send.is_empty() {
        // Typing puts the view back at the newest output, which is what every terminal does.
        session.scroll_to_bottom();
        for bytes in to_send {
            session.send(bytes);
        }
    }
    // **A key sent to the program is taken out of the frame**, or whatever is drawn after the terminal reads the
    // same press. Escape pressed at an agent's prompt reached the program *and* closed the ticket modal around
    // it; Enter reached the program *and* opened whatever the board's ring was on. Three keys, because these are
    // the ones something else in Quill also listens for: Escape closes every modal, Enter confirms one and opens
    // a ticket, and Tab moves between a window's controls. The letters are not consumed, because nothing else is
    // listening for a letter while a terminal has the keyboard.
    ui.ctx().input_mut(|input| {
        for key in [egui::Key::Escape, egui::Key::Enter, egui::Key::Tab] {
            input.consume_key(egui::Modifiers::NONE, key);
        }
    });
}

/// What a clipboard key press means to the terminal.
enum Clipboard {
    /// Put this on the clipboard, and let go of the selection it came from.
    Copy(String),
    /// Give these bytes to the program.
    Send(Vec<u8>),
    Nothing,
}

/// Whether a copy or a cut in the terminal copies text or reaches the program.
///
/// `task-1671` reported that `Ctrl+C` could not stop a command. The encoding was never the problem —
/// `keys::encode` has turned `Ctrl+C` into `0x03` since the terminal was written, and there is a test
/// of it. **The key press never arrived.** Before `egui-winit` pushes a key event it asks whether the
/// press is a clipboard command, and `is_copy_command` is `modifiers.command && key == C`. On macOS
/// `command` is the Apple key, so `Ctrl+C` is an ordinary key press there and always worked. On
/// Windows `command` *is* the control key, so every `Ctrl+C` became an `Event::Copy` with no key
/// event and no text event behind it, and the terminal copied the selection instead — which with
/// nothing selected meant it did nothing at all.
///
/// So the choice is made here, the way every terminal on Windows makes it:
///
/// - Something is selected, and `Ctrl+C` copies it. The selection is then let go of.
/// - Nothing is selected, and `Ctrl+C` interrupts the program.
/// - `Ctrl+Shift+C` copies whatever the state, which is the copy that never interrupts.
/// - `Ctrl+X` reaches the program as `0x18`, always. There is nothing in a terminal that can be cut,
///   and `Ctrl+X` is how a person leaves `nano`.
///
/// The control key has to be the one held for any of that. On macOS the Apple key is what makes
/// these events, and `Cmd+C` there means copy and nothing else. `Shift+Delete` and `Ctrl+Insert`
/// reach the window as the same two events and cannot be told from `Ctrl+X` and `Ctrl+C`, because
/// egui consumed the key; they take the copying half, which is what they meant on Windows anyway.
fn clipboard_key(
    cut: bool,
    selected: Option<String>,
    modifiers: &egui::Modifiers,
    mode: keys::Mode,
) -> Clipboard {
    let control = modifiers.ctrl && !modifiers.mac_cmd;
    let to_the_program = |letter: char| {
        let press = KeyPress::new(keys::Key::Character(letter), TermModifiers::control());
        match keys::encode(press, mode) {
            Some(bytes) => Clipboard::Send(bytes),
            None => Clipboard::Nothing,
        }
    };
    if control && cut {
        return to_the_program('x');
    }
    if control && !modifiers.shift && selected.is_none() {
        return to_the_program('c');
    }
    match selected {
        Some(text) => Clipboard::Copy(text),
        None => Clipboard::Nothing,
    }
}

/// Turn one of egui's key presses into a terminal key press, or nothing when the key is one the text event
/// carries instead.
fn key_press(key: egui::Key, modifiers: &egui::Modifiers) -> Option<KeyPress> {
    use quill_terminal::keys::Key as TermKey;
    let control = modifiers.ctrl && !modifiers.mac_cmd;
    let terminal_modifiers = TermModifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        control,
        command: modifiers.mac_cmd || (modifiers.command && !control),
    };
    let terminal_key = match key {
        egui::Key::Enter => TermKey::Enter,
        egui::Key::Backspace => TermKey::Backspace,
        egui::Key::Tab => TermKey::Tab,
        egui::Key::Escape => TermKey::Escape,
        egui::Key::ArrowUp => TermKey::Up,
        egui::Key::ArrowDown => TermKey::Down,
        egui::Key::ArrowLeft => TermKey::Left,
        egui::Key::ArrowRight => TermKey::Right,
        egui::Key::Home => TermKey::Home,
        egui::Key::End => TermKey::End,
        egui::Key::PageUp => TermKey::PageUp,
        egui::Key::PageDown => TermKey::PageDown,
        egui::Key::Insert => TermKey::Insert,
        egui::Key::Delete => TermKey::Delete,
        egui::Key::F1 => TermKey::Function(1),
        egui::Key::F2 => TermKey::Function(2),
        egui::Key::F3 => TermKey::Function(3),
        egui::Key::F4 => TermKey::Function(4),
        egui::Key::F5 => TermKey::Function(5),
        egui::Key::F6 => TermKey::Function(6),
        egui::Key::F7 => TermKey::Function(7),
        egui::Key::F8 => TermKey::Function(8),
        egui::Key::F9 => TermKey::Function(9),
        egui::Key::F10 => TermKey::Function(10),
        egui::Key::F11 => TermKey::Function(11),
        egui::Key::F12 => TermKey::Function(12),
        other => {
            // An ordinary key. Only sent from here when control or alt is held, because otherwise egui has
            // already reported it as text, which is the version that respects the keyboard layout.
            if !control && !modifiers.alt {
                return None;
            }
            TermKey::Character(symbol(other, modifiers.shift)?)
        }
    };
    Some(KeyPress::new(terminal_key, terminal_modifiers))
}

/// The character an ordinary key stands for, as a terminal reads it, or nothing when it stands for no
/// character this key press can be turned into.
///
/// This used to ask `actions::key_name`, which is what a **menu** shows and so spells the punctuation as
/// words — `Backslash`, `OpenBracket`, `Semicolon` — because `Ctrl+Backslash` reads better in a menu than
/// `Ctrl+\`. A word is not one character, so every one of those keys fell out and was sent as nothing at
/// all. `Ctrl+]` is how a person detaches from `claude`, `Ctrl+\` is how a program is quit and
/// `Ctrl+Space` is how a null is sent, and in Quill's terminal all three did nothing.
///
/// **A digit or a piece of punctuation held with shift is a different character**, and which one depends
/// on the keyboard: `Shift+4` is `$` here and `"` on a British layout. So there is no control code to be
/// had from the digit, and taking the digit's own was how the stray `^\` on the prompt in `task-1670`
/// got there — that is 0x1c, which is what `4` makes, and `Ctrl+Shift+4` is a screenshot on this
/// machine. A letter is not affected: shift does not change which letter it is, and `Ctrl+Shift+C` is
/// `Ctrl+C` in every terminal there is.
fn symbol(key: egui::Key, shift: bool) -> Option<char> {
    // Two are named here rather than taken from egui. The minus key, because egui spells it with the
    // typographic minus sign at U+2212 rather than the hyphen a shell reads; and space, whose name is a
    // word. Everything else — the letters, the digits and the punctuation — egui already gives as the
    // character itself.
    let character = match key {
        egui::Key::Minus => '-',
        egui::Key::Space => ' ',
        other => {
            let text = other.symbol_or_name();
            let mut characters = text.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                // A key whose name is a word, such as `Escape`, which is handled above; anything else
                // left is not a character a terminal can send.
                return None;
            }
            character.to_ascii_lowercase()
        }
    };
    if shift && !character.is_ascii_alphabetic() {
        return None;
    }
    Some(character)
}

/// The height a tile with `rows` rows of text needs, which the window uses to open the tile at a sensible
/// height and a test uses to know what to expect.
pub fn height_for(rows: usize, cell_height: f32) -> f32 {
    HEADER + 2.0 + PADDING_Y * 2.0 + rows as f32 * cell_height
}

/// The size of the grid a tile of this size holds, which is the arithmetic in one place so that the tile,
/// the emulator and the program on the far side cannot disagree.
pub fn grid_size(tile: Vec2, cell: crate::services::text_renderer::CellMetrics) -> Size {
    let width = tile.x - PADDING_X * 2.0;
    let height = tile.y - HEADER - 2.0 - PADDING_Y * 2.0;
    let columns = (width / cell.width).floor().max(1.0) as usize;
    let rows = (height / cell.height).floor().max(1.0) as usize;
    Size::new(rows, columns).with_cell(cell.width, cell.height)
}

/// Space for the tile's own furniture, used when the window works out how tall to open it.
pub const FURNITURE: f32 = HEADER + 2.0 + PADDING_Y * 2.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::text_renderer::CellMetrics;

    #[test]
    fn the_grid_is_as_many_whole_cells_as_fit() {
        let cell = CellMetrics { width: 10.0, height: 20.0, ascent: 15.0 };
        let size = grid_size(Vec2::new(10.0 * 8.0 + PADDING_X * 2.0, FURNITURE + 20.0 * 5.0), cell);
        assert_eq!(size.columns, 8);
        assert_eq!(size.rows, 5);
    }

    #[test]
    fn a_tile_too_small_for_one_cell_still_has_a_grid_of_one() {
        let cell = CellMetrics { width: 10.0, height: 20.0, ascent: 15.0 };
        let size = grid_size(Vec2::new(1.0, 1.0), cell);
        assert_eq!((size.rows, size.columns), (1, 1), "a grid of nothing would divide by zero later");
    }

    #[test]
    fn the_height_for_a_number_of_rows_and_the_grid_that_fits_it_agree() {
        let cell = CellMetrics { width: 8.0, height: 17.0, ascent: 13.0 };
        let height = height_for(12, cell.height);
        let size = grid_size(Vec2::new(400.0, height), cell);
        assert_eq!(size.rows, 12, "asking for twelve rows should give a tile that holds twelve");
    }

    #[test]
    fn a_letter_with_no_modifier_is_left_to_the_text_event() {
        let plain = egui::Modifiers::default();
        assert!(
            key_press(egui::Key::A, &plain).is_none(),
            "egui reports the letter as text, which is the version that respects the keyboard layout"
        );
    }

    #[test]
    fn control_and_a_letter_becomes_a_terminal_key_press() {
        let control = egui::Modifiers { ctrl: true, ..Default::default() };
        let press = key_press(egui::Key::C, &control).expect("Control C should be sent");
        assert_eq!(press.key, quill_terminal::keys::Key::Character('c'));
        assert!(press.modifiers.control);
        let bytes = keys::encode(press, keys::Mode::default()).expect("it sends something");
        assert_eq!(bytes, vec![0x03], "which is what stops a program");
    }

    #[test]
    fn the_special_keys_are_all_carried_across() {
        let plain = egui::Modifiers::default();
        for (key, expected) in [
            (egui::Key::Enter, quill_terminal::keys::Key::Enter),
            (egui::Key::Tab, quill_terminal::keys::Key::Tab),
            (egui::Key::Escape, quill_terminal::keys::Key::Escape),
            (egui::Key::ArrowUp, quill_terminal::keys::Key::Up),
            (egui::Key::Delete, quill_terminal::keys::Key::Delete),
            (egui::Key::F5, quill_terminal::keys::Key::Function(5)),
        ] {
            let press = key_press(key, &plain).unwrap_or_else(|| panic!("{key:?} should be sent"));
            assert_eq!(press.key, expected);
        }
    }

    #[test]
    fn control_and_a_piece_of_punctuation_reaches_the_program() {
        // `task-1670`. These all fell out before, because the name a menu shows for them is a word:
        // `Ctrl+]` detaches from `claude`, `Ctrl+\` quits a program and `Ctrl+Space` sends a null.
        let control = egui::Modifiers { ctrl: true, ..Default::default() };
        for (key, expected) in [
            (egui::Key::CloseBracket, 0x1d_u8),
            (egui::Key::OpenBracket, 0x1b),
            (egui::Key::Backslash, 0x1c),
            (egui::Key::Space, 0x00),
            (egui::Key::Minus, 0x1f),
        ] {
            let press =
                key_press(key, &control).unwrap_or_else(|| panic!("Control and {key:?} is a key press"));
            let bytes = keys::encode(press, keys::Mode::default())
                .unwrap_or_else(|| panic!("Control and {key:?} should send something"));
            assert_eq!(bytes, vec![expected], "Control and {key:?}");
        }
    }

    #[test]
    fn control_and_a_shifted_digit_sends_nothing() {
        // A shifted digit is punctuation, and which punctuation depends on the layout, so the digit's
        // own control code is not the answer. 0x1c is what `4` makes, and it is the stray `^\` on the
        // prompt in `task-1670` — `Ctrl+Shift+4` takes a screenshot on that machine.
        let control_shift = egui::Modifiers { ctrl: true, shift: true, ..Default::default() };
        assert!(key_press(egui::Key::Num4, &control_shift).is_none(), "Ctrl+Shift+4 is Ctrl+$");
        assert!(key_press(egui::Key::Num3, &control_shift).is_none());

        // The digit on its own still is: `Ctrl+4` is 0x1c in every terminal.
        let control = egui::Modifiers { ctrl: true, ..Default::default() };
        let press = key_press(egui::Key::Num4, &control).expect("Ctrl+4 is a key press");
        assert_eq!(keys::encode(press, keys::Mode::default()), Some(vec![0x1c]));

        // And a letter is not affected, because shift does not change which letter it is.
        let press = key_press(egui::Key::C, &control_shift).expect("Ctrl+Shift+C is a key press");
        assert_eq!(keys::encode(press, keys::Mode::default()), Some(vec![0x03]));
    }

    /// What `clipboard_key` decided, as something a test can compare.
    fn decision(cut: bool, selected: Option<&str>, modifiers: egui::Modifiers) -> String {
        match clipboard_key(cut, selected.map(str::to_owned), &modifiers, keys::Mode::default()) {
            Clipboard::Copy(text) => format!("copy {text}"),
            Clipboard::Send(bytes) => format!("send {bytes:02x?}"),
            Clipboard::Nothing => "nothing".to_owned(),
        }
    }

    #[test]
    fn control_and_c_interrupts_the_program_when_nothing_is_selected() {
        // `task-1671`. This is the whole ticket: on Windows egui turns Ctrl+C into a copy event and
        // throws the key press away, so the terminal had no way to stop a command or leave `claude`.
        let control = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        assert_eq!(decision(false, None, control), "send [03]");
    }

    #[test]
    fn control_and_c_copies_when_something_is_selected_and_then_lets_go_of_it() {
        let control = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        assert_eq!(decision(false, Some("the output"), control), "copy the output");
        // Letting go is what the caller does with `Clipboard::Copy`, and it is what makes the second
        // press an interrupt rather than a second copy of the same thing.
        assert_eq!(decision(false, None, control), "send [03]");
    }

    #[test]
    fn control_and_shift_and_c_is_the_copy_that_never_interrupts() {
        let both = egui::Modifiers { ctrl: true, command: true, shift: true, ..Default::default() };
        assert_eq!(decision(false, Some("the output"), both), "copy the output");
        assert_eq!(decision(false, None, both), "nothing", "and it certainly does not interrupt");
    }

    #[test]
    fn control_and_x_reaches_the_program_whatever_is_selected() {
        // There is nothing in a terminal that can be cut, and Ctrl+X is how a person leaves `nano`.
        let control = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        assert_eq!(decision(true, None, control), "send [18]");
        assert_eq!(decision(true, Some("the output"), control), "send [18]");
    }

    #[test]
    fn the_apple_key_copies_and_never_interrupts() {
        // On macOS these events are made by Cmd+C and Cmd+X, and Ctrl+C arrives as an ordinary key
        // press instead — which is why the fault was Windows only.
        let apple = egui::Modifiers { command: true, mac_cmd: true, ..Default::default() };
        assert_eq!(decision(false, Some("the output"), apple), "copy the output");
        assert_eq!(decision(false, None, apple), "nothing");
        assert_eq!(decision(true, Some("the output"), apple), "copy the output");
    }

    #[test]
    fn nothing_is_sent_for_a_key_held_with_the_apple_key() {
        // Command and C copies what is selected in the terminal, and command and V pastes into it. Neither
        // reaches the program, and macOS sends no text for them either, so nothing is sent at all.
        let command = egui::Modifiers { command: true, mac_cmd: true, ..Default::default() };
        assert!(key_press(egui::Key::C, &command).is_none());
        assert!(key_press(egui::Key::V, &command).is_none());
        // An arrow key held with the Apple key is recognised, and marked so that it sends nothing.
        let press = key_press(egui::Key::ArrowLeft, &command).expect("an arrow is a key a terminal knows");
        assert!(press.modifiers.command);
        assert_eq!(keys::encode(press, keys::Mode::default()), None);
    }
}
