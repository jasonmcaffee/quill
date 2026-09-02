//! Drawing the Database plugin: the tree in the pane, and the consoles and grids in the tab.
//!
//! `_agent_output/task-1777-database-plugin/intellij/` is what this is measured against — the
//! Database tool window, the query console and the data editor, downloaded from JetBrains rather than
//! remembered — wearing the dark neumorphic palette the Agent-Tasks board is drawn in.
//!
//! ## The drawing changes nothing
//!
//! Every function here takes a rectangle, draws, and reports an [`Act`]. Not one of them changes a
//! connection, sends a statement or edits a row. [`pane`] and [`tab`] are the two places the acts are
//! applied, after everything has been drawn — which is `components::agent_chat`'s arrangement and the
//! rule `components::activity_bar` set long before either.
//!
//! ## The ground is the window's
//!
//! `show_the_plugin_panes` fills the pane and reserves the decoration's slot before this is called, so
//! nothing here paints a second ground: it would go into the painter *after* the slot and wash the
//! decoration out. What is painted here are the plugin's own surfaces, through `Chrome`.

pub mod console;
pub mod grid;
pub mod modal;
pub mod settings_page;
pub mod tree;
pub mod workspace;

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};

use crate::services::database::{commands, DatabaseExplorer, Modal, Page, Sheet};
use crate::services::plugin_ui::{Look, Request};
use crate::services::vello_canvas::{Fill, Lift};

/// The gap round a surface, which is the gap the explorer already leaves.
pub const PAD: f32 = 8.0;
/// A toolbar's height, which holds a 24 point icon button with 4 points either side.
pub const TOOLBAR: f32 = 32.0;
/// A corner radius for a card, from the board.
pub const RADIUS: f32 = 12.0;

/// What the drawing reported.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Open or close a data source's row in the tree.
    ToggleSource(String),
    ToggleSchema(String, String),
    ToggleFolder(String, String, String),
    ToggleTable(String, String, String),
    /// Choose a row, which is what the toolbar's buttons act on.
    Choose(String, String, String),
    /// Open a grid on a table.
    OpenTable(String, String, String),
    OpenConsole(String),
    Connect(String),
    Disconnect(String),
    Refresh(String),
    /// Show the `CREATE` statement for the chosen table.
    Ddl(String, String, String),
    /// The New Data Source modal, on a new one or on an existing one.
    NewSource,
    EditSource(String),
    /// Which page of the workspace is showing.
    ShowPage(u64),
    ClosePage(u64),
    /// A console.
    Execute(u64),
    Stop(u64),
    /// A grid.
    Reload(u64),
    Page(u64, usize),
    AddRow(u64),
    DeleteRow(u64, usize),
    RevertPending(u64),
    Preview(u64),
    Submit(u64),
    /// A cell was chosen, or typed into.
    ChooseCell(u64, usize, usize),
    SetCell(u64, usize, usize, quill_db::Value),
    /// Sort by a column, which sends a new `ORDER BY` rather than sorting the page.
    SortBy(u64, String),
    /// Say something in the status bar.
    Said(String),
}

/// Draw the tree pane, and act on what was pressed.
pub fn pane(explorer: &mut DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let mut acts = Vec::new();
    if area.width() > 40.0 && area.height() > 40.0 {
        acts = tree::show(explorer, ui, look, area);
    }
    apply(explorer, acts)
}

/// Draw the workspace tab, and act on what was pressed.
pub fn tab(explorer: &mut DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let mut acts = Vec::new();
    if area.width() > 80.0 && area.height() > 60.0 {
        acts = workspace::show(explorer, ui, look, area);
    }
    apply(explorer, acts)
}

/// The one place an act becomes a change.
pub fn apply(explorer: &mut DatabaseExplorer, acts: Vec<Act>) -> Vec<Request> {
    let mut requests = Vec::new();
    for act in acts {
        match act {
            Act::ToggleSource(name) => explorer.toggle_source(&name),
            Act::ToggleSchema(source, schema) => explorer.toggle_schema(&source, &schema),
            Act::ToggleFolder(source, schema, folder) => {
                explorer.toggle_folder(&source, &schema, &folder)
            }
            Act::ToggleTable(source, schema, name) => explorer.toggle_table(&source, &schema, &name),
            Act::Choose(source, schema, name) => {
                explorer.chosen = Some(crate::services::database::Chosen { source, schema, name });
            }
            Act::OpenTable(source, schema, name) => {
                if let Err(why) = explorer.open_table(&source, &schema, &name) {
                    requests.push(Request::Message(why));
                }
                requests.push(Request::ShowTab);
            }
            Act::OpenConsole(source) => match explorer.open_console(&source) {
                Ok(_) => requests.push(Request::ShowTab),
                Err(why) => requests.push(Request::Message(why)),
            },
            Act::Connect(name) => {
                if let Err(why) = explorer.connect(&name) {
                    requests.push(Request::Message(why));
                }
            }
            Act::Disconnect(name) => explorer.disconnect(&name),
            Act::Refresh(name) => {
                // A refresh is a disconnect and a reconnect, which is the honest form of "read it
                // again": every cached schema, item list and column list came down that connection.
                let was_open = explorer.open_sources.contains(&name);
                explorer.disconnect(&name);
                if was_open {
                    if let Err(why) = explorer.connect(&name) {
                        requests.push(Request::Message(why));
                    }
                }
            }
            Act::Ddl(source, schema, name) => {
                let kind = kind_of(explorer, &source, &schema, &name);
                if let Err(why) = explorer.ask_for_ddl(&source, &schema, &name, kind, true) {
                    requests.push(Request::Message(why));
                }
            }
            Act::NewSource => explorer.modal = Some(Modal::Source(commands::a_new_source(explorer))),
            Act::EditSource(name) => {
                if let Some(source) = explorer.configuration.source(&name) {
                    explorer.modal = Some(Modal::Source(commands::a_form_for(source)));
                }
            }
            Act::ShowPage(id) => {
                if let Some(at) = explorer.pages.iter().position(|page| page.id == id) {
                    explorer.current = at;
                }
            }
            Act::ClosePage(id) => explorer.close_page(id),
            Act::Execute(id) => match explorer.execute(id) {
                Ok(said) => requests.push(Request::Message(said)),
                Err(why) => requests.push(Request::Message(why)),
            },
            Act::Stop(id) => {
                if let Err(why) = explorer.stop(id) {
                    requests.push(Request::Message(why));
                }
            }
            Act::Reload(id) => explorer.reload(id),
            Act::Page(id, at) => {
                if let Some(Page { sheet: Sheet::Grid(grid), .. }) =
                    explorer.pages.iter_mut().find(|page| page.id == id)
                {
                    grid.at = at;
                }
                explorer.reload(id);
            }
            Act::AddRow(id) => with_grid(explorer, id, |grid| {
                grid.pending.add();
            }),
            Act::DeleteRow(id, at) => with_grid(explorer, id, |grid| {
                if let Some(row) = grid.row_of(at) {
                    grid.pending.delete(row);
                }
            }),
            Act::RevertPending(id) => with_grid(explorer, id, |grid| grid.pending.clear()),
            Act::Preview(id) => explorer.modal = Some(Modal::Preview { page: id }),
            Act::Submit(id) => {
                if let Err(why) = explorer.submit(id) {
                    requests.push(Request::Message(why));
                }
            }
            Act::ChooseCell(id, at, column) => with_grid(explorer, id, |grid| {
                grid.chosen = Some((at, column));
                grid.editing = None;
            }),
            Act::SetCell(id, at, column, value) => with_grid(explorer, id, |grid| {
                let Some(name) = grid.rows.columns.get(column).map(|column| column.name.clone()) else {
                    return;
                };
                if let Some(row) = grid.row_of(at) {
                    grid.pending.set(row, &name, value);
                }
            }),
            Act::SortBy(id, column) => {
                with_grid(explorer, id, |grid| {
                    // Click once for ascending, again for descending, again for none — which is what
                    // every grid does and what the chevron in the header draws.
                    let ascending = format!("{column} asc");
                    let descending = format!("{column} desc");
                    grid.order_by = match grid.order_by.trim() {
                        was if was == ascending => descending,
                        was if was == descending => String::new(),
                        _ => ascending,
                    };
                    grid.at = 0;
                });
                explorer.reload(id);
            }
            Act::Said(said) => requests.push(Request::Message(said)),
        }
    }
    requests
}

fn with_grid(explorer: &mut DatabaseExplorer, id: u64, work: impl FnOnce(&mut crate::services::database::Grid)) {
    if let Some(Page { sheet: Sheet::Grid(grid), .. }) = explorer.pages.iter_mut().find(|page| page.id == id) {
        work(grid);
    }
}

/// What kind of thing the tree says this is, defaulting to a table.
fn kind_of(explorer: &DatabaseExplorer, source: &str, schema: &str, name: &str) -> quill_db::Kind {
    explorer
        .loaded
        .get(source)
        .and_then(|loaded| loaded.items.get(schema))
        .and_then(|items| items.iter().find(|item| item.name == name))
        .map(|item| item.kind)
        .unwrap_or(quill_db::Kind::Table)
}

/// A raised card, or the flat bordered panel every list in Quill draws when the decoration is off.
///
/// One function rather than the same eight lines in four files, so switching `ui.chrome` off really
/// does leave a flat panel rather than leaving one surface half drawn.
pub fn card(ui: &egui::Ui, look: &Look<'_>, area: Rect, fill: Color32, radius: f32) {
    if look.chrome.is_recording() {
        look.chrome.raised(area, radius, Fill::Solid(fill), Lift::Small);
        return;
    }
    ui.painter().rect(
        area,
        CornerRadius::same(radius as u8),
        look.ground(fill),
        Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
}

/// A pressed well: a field, a results area, a toolbar's ground.
pub fn well(ui: &egui::Ui, look: &Look<'_>, area: Rect, radius: f32) {
    if look.chrome.is_recording() {
        look.chrome.sunken(area, radius, look.palette.board_well, Lift::Small);
        return;
    }
    ui.painter().rect(
        area,
        CornerRadius::same(radius as u8),
        look.ground(look.palette.board_well),
        Stroke::new(1.0, look.palette.divider),
        egui::StrokeKind::Inside,
    );
}

/// Text at a position, vertically centred in a row, cut to a width with an ellipsis.
///
/// Every list in this plugin draws its rows with it, so a long table name is cut the same way
/// everywhere rather than being clipped in one place and overflowing in another.
pub fn text(
    painter: &egui::Painter,
    at: Pos2,
    words: &str,
    tint: Color32,
    size: f32,
    width: f32,
) -> f32 {
    let mut galley = painter.layout_no_wrap(words.to_owned(), egui::FontId::proportional(size), tint);
    if galley.size().x > width && width > 12.0 {
        // Cut by characters rather than by bytes, so a name with an accent in it is not cut in half.
        let mut kept: String = words.to_owned();
        while !kept.is_empty() {
            kept.pop();
            let shorter = format!("{kept}…");
            galley = painter.layout_no_wrap(shorter, egui::FontId::proportional(size), tint);
            if galley.size().x <= width {
                break;
            }
        }
    }
    let height = galley.size().y;
    let width_drawn = galley.size().x;
    painter.galley(Pos2::new(at.x, at.y - height / 2.0), galley, tint);
    width_drawn
}

/// The same in the code font, for a value, a type or a statement.
pub fn code(
    painter: &egui::Painter,
    at: Pos2,
    words: &str,
    tint: Color32,
    size: f32,
    width: f32,
) -> f32 {
    let mut galley = painter.layout_no_wrap(words.to_owned(), egui::FontId::monospace(size), tint);
    if galley.size().x > width && width > 12.0 {
        let per = galley.size().x / words.chars().count().max(1) as f32;
        let fits = ((width - per) / per).max(0.0) as usize;
        let kept: String = words.chars().take(fits).collect();
        galley = painter.layout_no_wrap(format!("{kept}…"), egui::FontId::monospace(size), tint);
    }
    let height = galley.size().y;
    let drawn = galley.size().x;
    painter.galley(Pos2::new(at.x, at.y - height / 2.0), galley, tint);
    drawn
}

/// A spinner: three dots that fill in, for a source that is still answering.
///
/// Drawn rather than animated with a rotation, because the decoration canvas is only rasterised on a
/// changed frame and a spinning arc would rasterise on every one. `egui` paints these on top.
pub fn waiting(painter: &egui::Painter, centre: Pos2, tint: Color32, time: f64) {
    for index in 0..3 {
        let phase = (time * 2.0 + index as f64 * 0.3).sin() as f32;
        let alpha = (0.35 + 0.65 * (phase * 0.5 + 0.5)).clamp(0.0, 1.0);
        painter.circle_filled(
            Pos2::new(centre.x - 5.0 + index as f32 * 5.0, centre.y),
            1.6,
            tint.gamma_multiply(alpha),
        );
    }
}

/// The size of a rectangle, laid out left to right, for a toolbar.
pub fn along(area: Rect, at: &mut f32, width: f32) -> Rect {
    let rect = Rect::from_min_size(Pos2::new(*at, area.top()), Vec2::new(width, area.height()));
    *at += width;
    rect
}
