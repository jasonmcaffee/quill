//! The row editor: the toolbar, `WHERE` and `ORDER BY`, and the grid itself.
//!
//! The reference screenshots’ `data_editor_db_object_data.png` is the picture, and almost every decision here is read
//! straight off it: the row-number gutter, the header with a sort chevron per column, `WHERE` and
//! `ORDER BY` as *typed SQL* on one row under the toolbar, and the paging widget at the foot saying
//! `1-200 of 200+`.
//!
//! ## Three rules the drawing keeps
//!
//! **Only the rows on the screen are drawn**, which is `task-1666`'s rule: a page of two hundred rows
//! costs what twenty visible rows cost.
//!
//! **A hover does not change the decoration.** Moving the pointer across a grid is the commonest thing
//! anybody does in one, and re-rasterising the canvas for it would be the whole cost, all the time —
//! `task-1765` measured that. The hover is a wash `egui` paints on top.
//!
//! **Add, Delete and Submit are absent when the rows cannot be addressed**, with one line saying why.
//! That is the absent-control rule doing real work rather than decorating: the alternative is an
//! `UPDATE` matching on every column, which quietly changes two identical rows.

use egui::{Pos2, Rect, Sense, Vec2};

use unluminous_db::rows::Rows;
use unluminous_db::value::Value;

use crate::components::database::{along, code, text, well, Act, TOOLBAR};
use crate::services::database::{why_not, DatabaseExplorer, Page, Sheet};
use crate::services::plugin_ui::Look;
use crate::theme::{color, icon};

/// The gutter the row numbers are drawn in.
const GUTTER: f32 = 44.0;
/// How wide a column is. One width for all of them, which is what keeps a hundred-column table
/// scrollable rather than unreadable; the value editor is where a long value is looked at.
const COLUMN: f32 = 150.0;
/// The paging strip at the foot.
const FOOT: f32 = 26.0;

/// Draw one grid page.
pub fn show(
    explorer: &mut DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    id: u64,
) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), TOOLBAR * scale));
    acts.extend(toolbar(explorer, ui, look, bar, id));

    let filters = Rect::from_min_size(
        Pos2::new(area.left(), bar.bottom() + 4.0 * scale),
        Vec2::new(area.width(), 24.0 * scale),
    );
    acts.extend(where_and_order(explorer, ui, look, filters, id));

    let foot = Rect::from_min_max(Pos2::new(area.left(), area.bottom() - FOOT * scale), area.max);
    let body = Rect::from_min_max(
        Pos2::new(area.left(), filters.bottom() + 4.0 * scale),
        Pos2::new(area.right(), foot.top() - 4.0 * scale),
    );
    let Some(Page { sheet: Sheet::Grid(grid), .. }) = explorer.page(id) else { return acts };
    if let Some(why) = &grid.failure {
        let painter = ui.painter_at(body);
        let galley = painter.layout(
            why.to_string(),
            egui::FontId::monospace(look.monospace_size * 0.95),
            color::unsaved(),
            body.width(),
        );
        painter.galley(body.min + Vec2::splat(8.0 * scale), galley, color::unsaved());
        return acts;
    }
    let rows = grid.rows.clone();
    acts.extend(the_grid(explorer, ui, look, body, id, &rows));
    acts.extend(paging(explorer, ui, look, foot, id));
    acts
}

/// The buttons of `data_editor_db_object_data.png` that apply.
fn toolbar(
    explorer: &DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    bar: Rect,
    id: u64,
) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let Some(Page { sheet: Sheet::Grid(grid), .. }) = explorer.page(id) else { return acts };
    let running = grid.running.is_some();
    let editable = why_not(grid).is_none();
    let pending = grid.pending.len();
    let chosen_row = grid.chosen.map(|(row, _)| row);
    let step = 26.0 * scale;
    let mut at = bar.left();

    if crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Reload", icon::rerun) {
        acts.push(Act::Reload(id));
    }
    if running && crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Stop", icon::stop) {
        acts.push(Act::Stop(id));
    }
    if editable {
        at += 6.0 * scale;
        if crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Add row", icon::plus) {
            acts.push(Act::AddRow(id));
        }
        if let Some(row) = chosen_row {
            if crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Delete row", icon::bin) {
                acts.push(Act::DeleteRow(id, row));
            }
        }
        // Set NULL is here because an empty box cannot mean it: a cell is edited as its value, and a
        // box that opened with the word `NULL` in it would write those four letters back. See
        // `Grid::text_of`.
        if grid.chosen.is_some()
            && crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Set NULL", icon::cross)
        {
            acts.push(Act::NullTheCell(id));
        }
        if pending > 0 {
            if crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Revert", icon::undo) {
                acts.push(Act::RevertPending(id));
            }
            if crate::components::controls::icon_button(ui, along(bar, &mut at, step), "Preview pending changes", icon::copy) {
                acts.push(Act::Preview(id));
            }
            // **`Save`**, which is the word `task-1795` asks for: *"see a save button that writes to
            // the table for that value"*. It is the same call `submit` has always been.
            let save = Rect::from_min_size(
                Pos2::new(at + 6.0 * scale, bar.top() + 3.0 * scale),
                Vec2::new(78.0 * scale, bar.height() - 6.0 * scale),
            );
            at = save.right();
            if crate::components::modal::button(ui, save, &format!("Save {pending}"), !running, true) {
                acts.push(Act::Submit(id));
            }
        }
    }
    let _ = at;
    // Why the editing controls are absent, said once rather than left to be guessed at.
    if let Some(why) = why_not(grid) {
        text(
            ui.painter(),
            Pos2::new(bar.left() + 60.0 * scale, bar.center().y),
            &why,
            color::text_faint(),
            look.font_size * 0.8,
            bar.width() - 70.0 * scale,
        );
    }
    acts
}

/// The two fields the reference editor puts under the toolbar, each a fragment of SQL somebody types.
fn where_and_order(
    explorer: &mut DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    id: u64,
) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let half = area.width() / 2.0 - 4.0 * scale;
    let left = Rect::from_min_size(area.min, Vec2::new(half, area.height()));
    let right = Rect::from_min_size(Pos2::new(area.left() + half + 8.0 * scale, area.top()), Vec2::new(half, area.height()));
    well(ui, look, left, 6.0 * scale);
    well(ui, look, right, 6.0 * scale);
    let Some(page) = explorer.pages.iter_mut().find(|page| page.id == id) else { return acts };
    let Sheet::Grid(grid) = &mut page.sheet else { return acts };
    let where_response = fragment_field(ui, look, left, "WHERE", &mut grid.where_clause);
    let order_response = fragment_field(ui, look, right, "ORDER BY", &mut grid.order_by);
    // Enter sends it, which is what a filter field does everywhere else in Unluminous. Typing alone must
    // not, or every keystroke would be a round trip to the server.
    if (where_response.lost_focus() || order_response.lost_focus())
        && ui.input(|input| input.key_pressed(egui::Key::Enter))
    {
        grid.at = 0;
        acts.push(Act::Reload(id));
    }
    acts
}

/// A field holding a fragment of SQL.
///
/// **Not `controls::search_field`**, which draws a magnifier: this is not a search, it is a clause,
/// and a magnifier in front of `WHERE` says the wrong thing about what pressing Enter will do. The
/// word itself is the label, in small spaced capitals, which is `modal::section`'s own treatment for
/// a name a person reads rather than types.
fn fragment_field(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    name: &str,
    value: &mut String,
) -> egui::Response {
    let scale = look.scale();
    let painter = ui.painter().clone();
    let label_width = painter
        .layout_no_wrap(name.to_owned(), egui::FontId::proportional(look.font_size * 0.75), color::text_faint())
        .size()
        .x;
    text(
        &painter,
        Pos2::new(area.left() + 8.0 * scale, area.center().y),
        name,
        color::text_faint(),
        look.font_size * 0.75,
        label_width + 2.0,
    );
    let text_rect = Rect::from_min_max(
        Pos2::new(area.left() + 16.0 * scale + label_width, area.top() + 2.0 * scale),
        Pos2::new(area.right() - 6.0 * scale, area.bottom() - 2.0 * scale),
    );
    // The whole well, including the word in front of it, hands the keyboard to the box.
    let fragment_id = egui::Id::new(("database-fragment-field", name));
    crate::components::controls::claim_the_field(ui, area, fragment_id);
    let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).id_salt(("database-fragment", name)));
    let response = edit.add(
        egui::TextEdit::singleline(value)
            .id(fragment_id)
            .frame(egui::Frame::NONE)
            .font(egui::FontId::monospace(look.monospace_size * 0.95))
            .desired_width(text_rect.width())
            .text_color(color::text_control()),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
    response
}

/// The header, the gutter and the cells.
fn the_grid(
    explorer: &mut DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    id: u64,
    rows: &Rows,
) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    well(ui, look, area, 8.0 * scale);
    if rows.columns.is_empty() {
        text(
            ui.painter(),
            Pos2::new(area.left() + 10.0 * scale, area.top() + 16.0 * scale),
            "No rows.",
            color::text_faint(),
            look.font_size * 0.9,
            area.width(),
        );
        return acts;
    }
    let head = Rect::from_min_size(area.min, Vec2::new(area.width(), 24.0 * scale));
    let body = Rect::from_min_max(Pos2::new(area.left(), head.bottom()), area.max);
    let column_width = COLUMN * scale;
    let row_height = look.row_height * 0.9;
    // The rows that were read **and** the rows that have been added here. A pending insert is not in
    // `rows.rows`, so before `task-1795` pressing `Add row` changed a count and put nothing on the
    // screen.
    let drawn_rows = match explorer.page(id) {
        Some(Page { sheet: Sheet::Grid(grid), .. }) => grid.row_count(),
        _ => rows.rows.len(),
    };

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(("database-grid", id)));
    let out = egui::ScrollArea::both()
        .id_salt(("database-grid-rows", id))
        .max_height(body.height())
        .auto_shrink([false, false])
        .show_rows(&mut child, row_height, drawn_rows, |ui, range| {
            let mut acts = Vec::new();
            for at in range {
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(GUTTER * scale + column_width * rows.columns.len() as f32, row_height),
                    Sense::hover(),
                );
                acts.extend(one_row(explorer, ui, look, rect, id, at, rows, column_width, scale));
            }
            acts
        });
    acts.extend(out.inner);

    // The header is drawn **after** the rows and over the same x offset, so it stays put while the
    // rows scroll under it — which is what a grid does and what a header inside the scrolling area
    // could not do.
    header(explorer, ui, look, head, id, rows, column_width, out.state.offset.x, &mut acts);
    acts
}

/// The column names, with their types and a sort chevron.
#[allow(clippy::too_many_arguments)]
fn header(
    explorer: &DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    head: Rect,
    id: u64,
    rows: &Rows,
    column_width: f32,
    offset: f32,
    acts: &mut Vec<Act>,
) {
    let scale = look.scale();
    let order = match explorer.page(id) {
        Some(Page { sheet: Sheet::Grid(grid), .. }) => grid.order_by.clone(),
        _ => String::new(),
    };
    let painter = ui.painter_at(head);
    painter.rect_filled(head, egui::CornerRadius::same(6), look.palette.board_lane);
    for (index, column) in rows.columns.iter().enumerate() {
        let left = head.left() + GUTTER * scale + index as f32 * column_width - offset;
        if left > head.right() || left + column_width < head.left() {
            continue;
        }
        let rect = Rect::from_min_size(Pos2::new(left, head.top()), Vec2::new(column_width, head.height()));
        let response = ui.interact(rect, ui.id().with(("database-column", id, index)), Sense::click());
        if response.hovered() {
            painter.rect_filled(rect, egui::CornerRadius::same(4), look.palette.control);
        }
        let tint = match column.in_key {
            true => color::accent(),
            false => color::text_strong(),
        };
        let drawn = text(
            &painter,
            Pos2::new(rect.left() + 6.0 * scale, rect.center().y),
            &column.name,
            tint,
            look.font_size * 0.85,
            column_width - 34.0 * scale,
        );
        if !column.type_name.is_empty() {
            text(
                &painter,
                Pos2::new(rect.left() + 10.0 * scale + drawn, rect.center().y),
                &column.type_name,
                color::text_faint(),
                look.font_size * 0.7,
                column_width - drawn - 34.0 * scale,
            );
        }
        // The chevron says which way this column is sorted, and nothing at all when it is not — which
        // is how a grid says "the order you are looking at is the server's own".
        let sorting = order.trim();
        if sorting == format!("{} asc", column.name) {
            icon::disclosure_at(&painter, Pos2::new(rect.right() - 10.0 * scale, rect.center().y), true, color::accent(), scale);
        } else if sorting == format!("{} desc", column.name) {
            icon::disclosure_at(&painter, Pos2::new(rect.right() - 10.0 * scale, rect.center().y), false, color::accent(), scale);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &column.name));
        if response.clicked() {
            acts.push(Act::SortBy(id, column.name.clone()));
        }
    }
}

/// One row: its number in the gutter, then its cells.
#[allow(clippy::too_many_arguments)]
fn one_row(
    explorer: &DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    rect: Rect,
    id: u64,
    at: usize,
    rows: &Rows,
    column_width: f32,
    scale: f32,
) -> Vec<Act> {
    let mut acts = Vec::new();
    let Some(Page { sheet: Sheet::Grid(grid), .. }) = explorer.page(id) else { return acts };
    let deleted = grid.row_of(at).is_some_and(|row| grid.pending.is_deleted(&row));
    let added = at >= grid.rows.rows.len();
    let editing = grid.editing.clone().filter(|editing| editing.at == at);
    // Cloned rather than borrowed from the `Ui`: the cell editor below wants the `Ui` mutably, and a
    // painter borrowed from it would still be alive at that point.
    let painter = ui.painter().clone();
    // The row number, in the gutter, which is what tells `set 3 name Ada` which row it means. A row
    // that was added here is `+1`, `+2` in the accent colour, because it is not in the database yet
    // and a number that looked like the others would say it was.
    let (number, tint) = match added {
        true => (format!("+{}", at - grid.rows.rows.len() + 1), color::accent()),
        false => ((at + 1).to_string(), color::text_faint()),
    };
    text(
        &painter,
        Pos2::new(rect.left() + 6.0 * scale, rect.center().y),
        &number,
        tint,
        look.font_size * 0.75,
        GUTTER * scale - 12.0 * scale,
    );
    for (index, column) in rows.columns.iter().enumerate() {
        let left = rect.left() + GUTTER * scale + index as f32 * column_width;
        let cell = Rect::from_min_size(Pos2::new(left, rect.top()), Vec2::new(column_width, rect.height()));
        if !ui.clip_rect().intersects(cell) {
            continue;
        }
        let (value, is_pending) = grid.cell(at, index);
        let chosen = grid.chosen == Some((at, index));
        let open = editing.as_ref().is_some_and(|editing| editing.column == index);
        if open {
            acts.extend(cell_editor(ui, look, cell, id, at, index, &column.name, editing.as_ref()));
            continue;
        }
        let response = ui.interact(cell, ui.id().with(("database-cell", id, at, index)), Sense::click());
        if chosen {
            painter.rect_filled(cell.shrink(1.0), egui::CornerRadius::same(3), look.palette.selected_row);
        } else if is_pending {
            painter.rect_filled(cell.shrink(1.0), egui::CornerRadius::same(3), look.palette.modified.gamma_multiply(0.25));
        } else if response.hovered() {
            painter.rect_filled(cell.shrink(1.0), egui::CornerRadius::same(3), look.palette.control);
        }
        draw_a_value(&painter, look, cell, &value, column.numeric, deleted, scale);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &format!("{} row {}", column.name, at + 1))
        });
        if response.clicked() {
            acts.push(Act::ChooseCell(id, at, index));
        }
        // **A double click opens the cell for typing**, which is `task-1795`. A single click chooses
        // it, as it always did, so the two gestures do not fight: choosing is what Delete row and
        // Set NULL act on.
        if response.double_clicked() {
            acts.push(Act::EditCell(id, at, index));
        }
    }
    acts
}

/// A cell that is open for typing.
///
/// The box is drawn over the cell and takes the keyboard the frame it opens, with everything in it
/// selected, so typing replaces the value and an arrow key does not. `Enter` commits, `Escape`
/// throws the typing away, and losing the keyboard commits as well — which is what clicking another
/// cell does, and is the one behaviour a grid has to get right or an edit is lost by looking away.
#[allow(clippy::too_many_arguments)]
fn cell_editor(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    cell: Rect,
    id: u64,
    at: usize,
    column: usize,
    name: &str,
    editing: Option<&crate::services::database::Editing>,
) -> Vec<Act> {
    let mut acts = Vec::new();
    let Some(editing) = editing else { return acts };
    let scale = look.scale();
    ui.painter().rect(
        cell.shrink(1.0),
        egui::CornerRadius::same(3),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.accent),
        egui::StrokeKind::Inside,
    );
    let box_id = egui::Id::new(("database-cell-editor", id, at, column));
    let mut typed = editing.text.clone();
    let inner = Rect::from_min_max(
        Pos2::new(cell.left() + 4.0 * scale, cell.top() + 2.0 * scale),
        Pos2::new(cell.right() - 4.0 * scale, cell.bottom() - 2.0 * scale),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(box_id));
    let response = child.add(
        egui::TextEdit::singleline(&mut typed)
            .id(box_id)
            .frame(egui::Frame::NONE)
            .font(egui::FontId::monospace(look.monospace_size * 0.95))
            .desired_width(inner.width())
            .text_color(color::text_control()),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, &format!("{name} row {}", at + 1))
    });
    if typed != editing.text {
        acts.push(Act::TypeIntoCell(id, typed));
    }
    if editing.opened {
        response.request_focus();
        // Everything selected, so the first thing typed replaces the value rather than joining it.
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), box_id) {
            let whole = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(editing.text.chars().count()),
            );
            state.cursor.set_char_range(Some(whole));
            state.store(ui.ctx(), box_id);
        }
        acts.push(Act::OpenedTheCell(id));
    }
    if response.lost_focus() {
        match ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            true => acts.push(Act::CancelCell(id)),
            false => acts.push(Act::CommitCell(id)),
        }
    }
    acts
}

/// One cell's value.
///
/// **NULL is drawn as a dim `NULL` and never as text**, because a grid that showed NULL and the empty
/// string the same way is a grid nobody could trust — which is the reason `unluminous_db::Value` keeps them
/// apart all the way from the wire.
fn draw_a_value(
    painter: &egui::Painter,
    look: &Look<'_>,
    cell: Rect,
    value: &Value,
    numeric: bool,
    deleted: bool,
    scale: f32,
) {
    let size = look.monospace_size * 0.95;
    let room = cell.width() - 12.0 * scale;
    let tint = match (deleted, value.is_null()) {
        (true, _) => color::text_faint(),
        (false, true) => color::text_faint(),
        (false, false) => color::text(),
    };
    let said = match value.is_null() {
        true => "NULL".to_owned(),
        false => value.display().replace(['\n', '\r'], "⏎"),
    };
    let at = match numeric && !value.is_null() {
        // A number is right-aligned, which is the one piece of formatting the type decides.
        true => {
            let width = painter
                .layout_no_wrap(said.clone(), egui::FontId::monospace(size), tint)
                .size()
                .x
                .min(room);
            Pos2::new(cell.right() - 6.0 * scale - width, cell.center().y)
        }
        false => Pos2::new(cell.left() + 6.0 * scale, cell.center().y),
    };
    let drawn = code(painter, at, &said, tint, size, room);
    if deleted {
        // A row about to be deleted is struck through, so what Submit will do is visible without
        // pressing anything.
        painter.line_segment(
            [Pos2::new(at.x, cell.center().y), Pos2::new(at.x + drawn, cell.center().y)],
            egui::Stroke::new(1.0, color::text_faint()),
        );
    }
}

/// `|< < 1-200 of 200+ > >|`, which is the reference editor's own widget and its own honesty about the count.
fn paging(
    explorer: &DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    foot: Rect,
    id: u64,
) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let Some(Page { sheet: Sheet::Grid(grid), .. }) = explorer.page(id) else { return acts };
    let size = explorer.configuration.page_size;
    let first = grid.at * size + 1;
    let last = grid.at * size + grid.rows.rows.len();
    let said = match (grid.rows.rows.is_empty(), grid.rows.more) {
        (true, _) => "no rows".to_owned(),
        // `of N+` rather than `of N`, because nobody counted the rest: the statement asked for one
        // more row than it kept and that is all it knows.
        (false, true) => format!("{first}-{last} of {last}+"),
        (false, false) => format!("{first}-{last} of {last}"),
    };
    let mut at = foot.left();
    let step = 22.0 * scale;
    if grid.at > 0 {
        if crate::components::controls::icon_button(ui, along(foot, &mut at, step), "First page", icon::collapse) {
            acts.push(Act::Page(id, 0));
        }
        if crate::components::controls::icon_button(ui, along(foot, &mut at, step), "Previous page", icon::chevron_down) {
            acts.push(Act::Page(id, grid.at - 1));
        }
    }
    text(
        ui.painter(),
        Pos2::new(at + 8.0 * scale, foot.center().y),
        &said,
        color::text_dim(),
        look.font_size * 0.8,
        220.0 * scale,
    );
    at += 8.0 * scale + 160.0 * scale;
    if grid.rows.more
        && crate::components::controls::icon_button(ui, along(foot, &mut at, step), "Next page", icon::chevron_down)
    {
        acts.push(Act::Page(id, grid.at + 1));
    }
    acts
}

/// A result with no editing on it, which is what a console shows.
///
/// A console result is **never editable**: The reference editor tries to resolve one back to a table, and Unluminous
/// does not promise what it cannot enforce — a join has no one table behind it and guessing at which
/// is how the wrong row gets updated. The row editor is one click away and has the `WHERE` field.
pub fn rows_only(ui: &mut egui::Ui, look: &Look<'_>, area: Rect, rows: &Rows, id: u64) {
    let scale = look.scale();
    let column_width = COLUMN * scale;
    let row_height = look.row_height * 0.9;
    let head = Rect::from_min_size(area.min, Vec2::new(area.width(), 20.0 * scale));
    let painter = ui.painter_at(head);
    for (index, column) in rows.columns.iter().enumerate() {
        let left = head.left() + index as f32 * column_width;
        if left > head.right() {
            break;
        }
        text(
            &painter,
            Pos2::new(left + 4.0 * scale, head.center().y),
            &column.name,
            color::text_strong(),
            look.font_size * 0.8,
            column_width - 8.0 * scale,
        );
    }
    let body = Rect::from_min_max(Pos2::new(area.left(), head.bottom()), area.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(("database-result", id)));
    egui::ScrollArea::both()
        .id_salt(("database-result-rows", id))
        .max_height(body.height())
        .auto_shrink([false, false])
        .show_rows(&mut child, row_height, rows.rows.len(), |ui, range| {
            for at in range {
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(column_width * rows.columns.len().max(1) as f32, row_height),
                    Sense::hover(),
                );
                let painter = ui.painter();
                for (index, column) in rows.columns.iter().enumerate() {
                    let cell = Rect::from_min_size(
                        Pos2::new(rect.left() + index as f32 * column_width, rect.top()),
                        Vec2::new(column_width, rect.height()),
                    );
                    if !ui.clip_rect().intersects(cell) {
                        continue;
                    }
                    let value = rows.rows.get(at).and_then(|row| row.get(index)).cloned().unwrap_or_default();
                    draw_a_value(&painter, look, cell, &value, column.numeric, false, scale);
                }
            }
        });
}
