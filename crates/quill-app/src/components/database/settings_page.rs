//! The Database page in the Settings window.
//!
//! Built from `components::modal`'s own furniture — its sections, notes, fields, tick boxes and
//! buttons — so this is the same page as the other seven rather than an eighth that almost agrees with
//! them. It scrolls, which `components/agent_tasks/settings_page.rs` established for a page with more
//! in it than the 640 points every page gets.
//!
//! **No password is drawn and no password is written.** A row says where its password is — `environment
//! QUILL_DB_AI`, `keychain …`, `typed, until this window closes`, or `none` — and never what it is,
//! because a page that showed one is a page somebody screenshots.

use egui::{Pos2, Rect, Vec2};

use crate::components::database::{Act, PAD};
use crate::components::modal;
use crate::services::database::DatabaseExplorer;
use crate::services::plugin_ui::{Look, Request};
use crate::theme::color;

/// A row's height, and the gap under it.
const ROW: f32 = 26.0;
const GAP: f32 = 8.0;

/// Draw the page inside the rectangle every page gets.
pub fn show(explorer: &mut DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let height = ui.available_rect_before_wrap().height();
    let scrolled = egui::ScrollArea::vertical()
        .id_salt("database-settings")
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| rows(explorer, ui, look));
    scrolled.inner
}

/// The rows themselves.
///
/// The rectangle comes from the `Ui` this is given rather than from the caller, because inside a
/// scrolling area that `Ui`'s origin is where the scroll offset has put it — the fault
/// `components/agent_tasks/settings_page.rs` records, where the bar moved and the page did not.
fn rows(explorer: &mut DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let body = Rect::from_min_max(
        Pos2::new(area.left() + PAD * 1.5, area.top() + PAD * 1.5),
        Pos2::new(area.right() - PAD * 1.5, area.bottom()),
    );
    let mut requests = Vec::new();
    let mut acts = Vec::new();
    let mut pen = body.top();

    pen = modal::section(ui, body, pen, "Data sources");
    pen = modal::note(
        ui,
        body,
        pen,
        "Where each one points, and where its password is. Quill never writes a password down: a \
         source names an environment variable, or an entry in this machine's own keychain, or it \
         holds one you typed for as long as this window is open.",
    );
    let mut removing: Option<String> = None;
    let sources: Vec<quill_db::Source> = explorer.sources().to_vec();
    for source in &sources {
        pen = one_source(explorer, ui, look, body, pen, source, &mut acts, &mut removing);
    }
    if sources.is_empty() {
        pen = modal::note(ui, body, pen, "There are none yet.");
    }
    let add = Rect::from_min_size(Pos2::new(body.left(), pen + 4.0), Vec2::new(150.0, ROW));
    if modal::button(ui, add, "New data source", true, false) {
        acts.push(Act::NewSource);
    }
    pen = add.bottom() + GAP * 2.0;

    pen = modal::section(ui, body, pen, "Rows");
    let size_row = Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), ROW));
    let field = Rect::from_min_size(size_row.min, Vec2::new(90.0, ROW));
    let mut said = explorer.configuration.page_size.to_string();
    if modal::field(ui, field, "Rows a page", &mut said).changed() {
        if let Ok(size) = said.parse::<usize>() {
            explorer.configuration.page_size = size.clamp(1, 100_000);
            let _ = explorer.write_the_configuration();
        }
    }
    modal::label(
        ui.painter(),
        size_row,
        field.right() + 10.0,
        "rows a page, in a grid and in a console result",
        color::text_dim(),
        12.0,
    );
    pen = modal::note(
        ui,
        body,
        size_row.bottom() + 4.0,
        "One more than this is asked for and thrown away, which is what makes `1-200 of 200+` \
         honest: nobody counted the rest.",
    );

    pen = modal::section(ui, body, pen, "Safety");
    let confirm = Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), 22.0));
    let mut asking = explorer.configuration.confirm_writes;
    if modal::check(ui, confirm, "Ask before a console statement that changes rows", &mut asking) {
        explorer.configuration.confirm_writes = asking;
        let _ = explorer.write_the_configuration();
    }
    modal::note(
        ui,
        body,
        confirm.bottom() + 4.0,
        "A console is where somebody types `delete from member` meaning to type a `where` clause \
         after it. One dialog is cheaper than the row that is not there any more.",
    );

    if let Some(name) = removing {
        if let Err(why) = explorer.remove_source(&name) {
            requests.push(Request::Message(why));
        }
    }
    requests.extend(crate::components::database::apply(explorer, acts));
    requests
}

/// One data source's row: its name and buttons on one line, where it points on the next.
///
/// Two lines rather than one, because a SQLite file's path is long and a single line ran the path
/// underneath the Edit and Remove buttons. Everything is cut to the room there is, by the same
/// `text` that cuts a name in the tree.
#[allow(clippy::too_many_arguments)]
fn one_source(
    explorer: &DatabaseExplorer,
    ui: &mut egui::Ui,
    look: &Look<'_>,
    body: Rect,
    top: f32,
    source: &quill_db::Source,
    acts: &mut Vec<Act>,
    removing: &mut Option<String>,
) -> f32 {
    let _ = look;
    let row = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), ROW));
    let remove = Rect::from_min_size(Pos2::new(row.right() - 78.0, row.top()), Vec2::new(78.0, ROW));
    let edit = Rect::from_min_size(Pos2::new(remove.left() - 66.0, row.top()), Vec2::new(60.0, ROW));
    let painter = ui.painter_at(body);
    crate::components::database::text(
        &painter,
        Pos2::new(row.left(), row.center().y),
        &source.name,
        color::text_strong(),
        12.5,
        edit.left() - row.left() - 12.0,
    );
    if crate::components::modal::button(ui, edit, "Edit", true, false) {
        acts.push(Act::EditSource(source.name.clone()));
    }
    if crate::components::modal::button(ui, remove, "Remove", true, false) {
        *removing = Some(source.name.clone());
    }

    let under = Rect::from_min_size(Pos2::new(body.left(), row.bottom()), Vec2::new(body.width(), 18.0));
    let said = format!(
        "{} · {} · password {}",
        source.where_it_points(),
        match (explorer.is_connected(&source.name), source.read_only) {
            (true, true) => "connected, read only",
            (true, false) => "connected, writable",
            (false, true) => "read only",
            (false, false) => "writable",
        },
        source.secret.describe(),
    );
    crate::components::database::text(
        &painter,
        Pos2::new(under.left(), under.center().y),
        &said,
        color::text_faint(),
        11.0,
        under.width(),
    );
    under.bottom() + GAP
}
