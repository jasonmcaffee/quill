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
use crate::components::splitter;
use crate::services::text_renderer::TextRenderer;
use crate::theme::{color, icon};

/// How tall the strip holding the tabs is.
pub const HEADER: f32 = 32.0;
/// Space between the grid and the edge of the tile.
const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 6.0;

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
            tabs: Tabs::new(SessionSettings { shell: None, args: Vec::new(), working_directory }),
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
    /// How far the top edge was dragged, in points. Positive is downwards, which makes the tile shorter.
    pub drag: f32,
    /// The top edge was double clicked, which puts the tile back to its usual height.
    pub reset_height: bool,
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
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::TOOLBAR, opacity));

    // The top edge, which is dragged to change the height. Every pane in Quill is resized this way.
    let edge = Rect::from_min_size(area.left_top(), Vec2::new(area.width(), 1.0));
    let drag = splitter::show(ui, edge, "terminal", splitter::Axis::Flat);
    outcome.drag = drag.delta;
    outcome.reset_height = drag.reset;

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
    let painter = ui.painter_at(area);
    let heading = painter.layout_no_wrap(
        "Terminal".to_owned(),
        egui::FontId::proportional(12.0),
        color::TEXT_DIM,
    );
    painter.galley(
        Pos2::new(area.left() + 16.0, area.center().y - heading.size().y / 2.0),
        heading.clone(),
        color::TEXT_DIM,
    );

    let mut pen = area.left() + 16.0 + heading.size().x + 18.0;
    let names = panel.tabs.names();
    let active = panel.tabs.active_index();
    let mut show = None;
    let mut close = None;
    for (index, name) in names.iter().enumerate() {
        let label = painter.layout_no_wrap(
            name.clone(),
            egui::FontId::proportional(12.0),
            if index == active { color::TEXT_STRONG } else { color::TEXT_CONTROL },
        );
        let width = label.size().x + 38.0;
        let tab = Rect::from_min_size(
            Pos2::new(pen, area.center().y - 11.0),
            Vec2::new(width, 22.0),
        );
        let response = ui
            .interact(tab, ui.id().with(("terminal-tab", index)), Sense::click())
            .on_hover_text(format!("Terminal tab: {name}"));
        if index == active {
            painter.rect(
                tab,
                CornerRadius::same(4),
                color::SELECTED_ROW,
                Stroke::new(1.0, color::ACCENT.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(tab, CornerRadius::same(4), color::CONTROL);
        }
        painter.galley(
            Pos2::new(tab.left() + 10.0, tab.center().y - label.size().y / 2.0),
            label,
            color::TEXT_CONTROL,
        );
        let shut = Rect::from_center_size(
            Pos2::new(tab.right() - 12.0, tab.center().y),
            Vec2::splat(16.0),
        );
        let shut_response = ui
            .interact(shut, ui.id().with(("terminal-close", index)), Sense::click())
            .on_hover_text(format!("Close {name}"));
        icon::cross(&painter, shut.center(), color::TEXT_DIM);
        shut_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {name}"))
        });
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                index == active,
                format!("Terminal tab: {name}"),
            )
        });
        if shut_response.clicked() {
            close = Some(index);
        } else if response.clicked() {
            show = Some(index);
        }
        pen += width + 6.0;
    }
    if let Some(index) = close {
        panel.tabs.close(index);
        if panel.tabs.is_empty() {
            outcome.hide = true;
        }
    } else if let Some(index) = show {
        panel.tabs.show(index);
        outcome.take_focus = true;
    }

    let add = Rect::from_center_size(Pos2::new(pen + 11.0, area.center().y), Vec2::splat(22.0));
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

/// The grid: work out its size, tell the session, draw what it reports, and take the input.
fn show_grid(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut TerminalPanel,
    renderer: &TextRenderer,
    font_size: f32,
    opacity: f32,
    outcome: &mut PanelOutcome,
) {
    let cell = renderer.cell_metrics(font_size);
    let columns = ((area.width() - PADDING_X * 2.0) / cell.width).floor().max(1.0) as usize;
    let rows = ((area.height() - PADDING_Y * 2.0) / cell.height).floor().max(1.0) as usize;
    let wanted = Size::new(rows, columns).with_cell(cell.width, cell.height);

    // Everything below the tabs takes clicks, so a click anywhere in the grid moves the keyboard to the
    // terminal.
    let response = ui.interact(area, ui.id().with("terminal-grid"), Sense::click_and_drag());

    let Some(session) = panel.tabs.active_mut() else {
        // No tab open: say so rather than leaving an empty tile, and say why if a shell would not start.
        let message = panel
            .tabs
            .last_error
            .clone()
            .unwrap_or_else(|| "No terminal. Press the plus to open one.".to_owned());
        let painter = ui.painter_at(area);
        let galley = painter.layout_no_wrap(
            message,
            egui::FontId::proportional(12.0),
            color::TEXT_FAINT,
        );
        painter.galley(
            Pos2::new(area.left() + PADDING_X + 4.0, area.top() + PADDING_Y + 6.0),
            galley,
            color::TEXT_FAINT,
        );
        return;
    };

    // Told once, and told to both the emulator and the program on the far side.
    session.resize(wanted);
    session.pump();
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
    paint(&painter_ui, renderer, &screen, origin, cell, font_size, panel.focused, opacity);

    // Input, once the drawing is decided, so that a key press is acted on with the size the program already
    // knows about.
    handle_input(ui, panel, &response, &screen, origin, cell, outcome);
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
            painter.rect_filled(rect, CornerRadius::ZERO, color::TEXT_SELECTION);
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
            painter.rect_filled(shape, CornerRadius::ZERO, color::ACCENT);
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
                Stroke::new(1.0, color::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }

    // How far back the view is scrolled, said in words in the corner, because a terminal with no scroll bar
    // otherwise gives no sign that there is more above.
    if screen.scrollback > 0 {
        let text = format!("{} lines back", screen.scrollback);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color::TEXT_DIM);
        let at = Pos2::new(
            ui.max_rect().right() - 12.0 - galley.size().x,
            ui.max_rect().top() + 6.0,
        );
        painter.rect_filled(
            Rect::from_min_size(at, galley.size()).expand(4.0),
            CornerRadius::same(4),
            color::MENU,
        );
        painter.galley(at, galley, color::TEXT_DIM);
    }
}

/// The keyboard and the mouse.
fn handle_input(
    ui: &mut egui::Ui,
    panel: &mut TerminalPanel,
    response: &egui::Response,
    screen: &Screen,
    origin: Pos2,
    cell: crate::services::text_renderer::CellMetrics,
    outcome: &mut PanelOutcome,
) {
    let modifiers = ui.input(|input| input.modifiers);
    let cell_at = |position: Pos2| -> (usize, usize) {
        let column = ((position.x - origin.x) / cell.width).floor().max(0.0) as usize;
        let row = ((position.y - origin.y) / cell.height).floor().max(0.0) as usize;
        (row.min(screen.rows.saturating_sub(1)), column.min(screen.columns.saturating_sub(1)))
    };

    // The mouse. A program that asked to be told about clicks is told; otherwise a drag selects text, and
    // holding shift always selects, which is the only way to copy out of such a program.
    let mouse_mode = panel.tabs.active().map(|session| session.mouse_mode()).unwrap_or_default();
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
            if let Some(bytes) = mouse::report(
                mouse_mode,
                kind,
                mouse::Button::Left,
                row,
                column,
                terminal_modifiers,
            ) {
                if let Some(session) = panel.tabs.active() {
                    session.send(bytes);
                }
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
                    if let Some(session) = panel.tabs.active() {
                        session.send(bytes);
                    }
                }
            }
        } else if let Some(session) = panel.tabs.active_mut() {
            if response.double_clicked() {
                session.begin_selection(row, column, SelectionKind::Word);
                panel.selecting = false;
            } else if response.drag_started() || (response.clicked() && !panel.selecting) {
                session.begin_selection(row, column, SelectionKind::Simple);
                panel.selecting = true;
            } else if response.dragged() {
                session.extend_selection(row, column);
            }
            if response.drag_stopped() {
                panel.selecting = false;
            }
        }
    }

    // The wheel. On the ordinary screen it moves the view through the history. On a program's own screen
    // there is no history, so it is sent as arrow keys when the program asked for that, and as a wheel
    // report when it asked about the mouse.
    let wheel = ui.input(|input| input.smooth_scroll_delta.y);
    if wheel.abs() > 0.5 && response.hovered() {
        let lines = (wheel / cell.height).round() as i32;
        let lines = if lines == 0 { wheel.signum() as i32 } else { lines };
        if let Some(session) = panel.tabs.active_mut() {
            if mouse_mode.reports_clicks() && !modifiers.shift {
                let button =
                    if lines > 0 { mouse::Button::WheelUp } else { mouse::Button::WheelDown };
                let (row, column) = response
                    .hover_pos()
                    .map(cell_at)
                    .unwrap_or((0, 0));
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
                    if let Some(bytes) =
                        keys::encode(KeyPress::plain(key), mode)
                    {
                        session.send(bytes);
                    }
                }
            } else {
                session.scroll(lines);
            }
        }
    }

    if !panel.focused {
        return;
    }
    // A box that takes typing, such as the explorer's filter, has been clicked into. It keeps the keyboard
    // until it is clicked away from, so the terminal stands aside: taking the keys here would leave the
    // filter box impossible to type in while the terminal was open.
    if ui.memory(|memory| memory.focused().is_some()) {
        return;
    }

    // The keyboard. egui's own focus is cleared while the terminal has it, so that Tab and Escape are sent
    // to the program instead of moving between the window's controls.
    ui.memory_mut(|memory| memory.stop_text_input());

    let events = ui.input(|input| input.events.clone());
    let mode = panel.tabs.active().map(|session| session.mode()).unwrap_or_default();
    let mut to_send: Vec<Vec<u8>> = Vec::new();
    let mut copy = None;
    for event in events {
        match event {
            // A copy while the terminal has the keyboard copies what is selected in it.
            egui::Event::Copy | egui::Event::Cut => {
                if let Some(session) = panel.tabs.active() {
                    copy = session.selected_text();
                }
            }
            egui::Event::Paste(text) => {
                to_send.push(keys::paste(&text, mode));
            }
            egui::Event::Text(text) => {
                // The text egui reports, rather than the letter worked out from the key, so that a keyboard
                // layout, a dead key and an input method all reach the program as what was typed.
                if !text.chars().any(char::is_control) {
                    to_send.push(text.into_bytes());
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                if let Some(press) = key_press(key, &modifiers) {
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
    if !to_send.is_empty() {
        if let Some(session) = panel.tabs.active_mut() {
            // Typing puts the view back at the newest output, which is what every terminal does.
            session.scroll_to_bottom();
            for bytes in to_send {
                session.send(bytes);
            }
        }
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
            let name = crate::app::actions::key_name(other);
            let mut characters = name.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                // A key whose name is a word, such as `Escape`, which is handled above; anything else left
                // is not a character a terminal can send.
                return None;
            }
            TermKey::Character(character.to_ascii_lowercase())
        }
    };
    Some(KeyPress::new(terminal_key, terminal_modifiers))
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
