//! The plugin's four modals, all built from `components::modal`'s own furniture.
//!
//! A tenth modal that drew its own header would be a tenth modal that almost agrees with the other
//! nine, so the frame, the header, the body, the footer and the buttons are the window's — and so are
//! the dragging and the resizing, which `modal::show` gives every dialog without asking.
//!
//! - **New Data Source** — the reference editor's General tab, cut to what applies: name, engine, host, port,
//!   database, user, where the password is, ssl mode, read only, and Test Connection reporting the
//!   server's own version string.
//! - **Preview pending changes** — the actual statements Submit will send, from the same call Submit
//!   makes, so the preview cannot drift from what happens.
//! - **DDL** — a `CREATE` statement.
//! - **Confirm** — a statement from a console that changes rows.

use egui::{Pos2, Rect, Vec2};

use unluminate_db::source::{Engine, Secret, Source, SslMode};

use crate::components::modal;
use crate::services::database::{ColumnForm, DatabaseExplorer, Modal, SourceForm, TableForm};
use crate::services::plugin_ui::{Look, Request};
use crate::theme::color;

/// How wide the label column is, so every field on the dialog starts at the same x.
///
/// A label column rather than a name inside each field, which is the reference editor's own arrangement and the
/// one thing the first version of this dialog got wrong: five unlabelled boxes told a person nothing
/// about which was the database and which was the user.
const LABEL: f32 = 112.0;
/// A field's own height, and the gap under a row.
const FIELD: f32 = 24.0;
const GAP: f32 = 6.0;

/// A label at the left, and the rectangle the field goes in, which starts past it.
fn labelled(ui: &egui::Ui, body: Rect, pen: f32, name: &str) -> Rect {
    let row = Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), FIELD));
    crate::components::modal::label(ui.painter(), row, row.left(), name, color::text_dim(), 12.0);
    Rect::from_min_max(Pos2::new(body.left() + LABEL, pen), Pos2::new(body.right(), pen + FIELD))
}

/// Draw whichever modal is open. Answers what it asked for and whether it closed.
pub fn show(explorer: &mut DatabaseExplorer, ctx: &egui::Context, look: &Look<'_>) -> (Vec<Request>, bool) {
    let Some(open) = explorer.modal.clone() else { return (Vec::new(), false) };
    match open {
        Modal::Source(form) => source_modal(explorer, ctx, look, form),
        Modal::Preview { page } => preview(explorer, ctx, page),
        Modal::Ddl { title, text } => reading(explorer, ctx, &format!("DDL — {title}"), &text),
        Modal::NewTable(form) => new_table(explorer, ctx, look, form),
    }
}

/// What the New Data Source dialog reported.
#[derive(Default)]
struct Outcome {
    close: bool,
    save: bool,
    test: bool,
}

/// The New Data Source dialog.
fn source_modal(
    explorer: &mut DatabaseExplorer,
    ctx: &egui::Context,
    look: &Look<'_>,
    mut form: SourceForm,
) -> (Vec<Request>, bool) {
    let _ = look;
    let mut requests = Vec::new();
    let mut outcome = Outcome::default();
    let heading = match form.was.is_empty() {
        true => "New Data Source",
        false => "Data Source",
    };
    // **The heights add up**, which is the whole of a modal's difficulty here: the rows are laid out
    // one under another with no scroll, so a budget that overflows draws the last of them over the
    // buttons — which is exactly what the first version of this dialog did. 470 is what the rows below
    // need, counted rather than guessed: `task-1795` took the whole Password section and the whole
    // Safety section away and put one Password field under User instead.
    let (_, escaped) = modal::show(ctx, "unluminate-database-source", 520.0, 470.0, |ui, area| {
        if modal::header(ui, area, heading) {
            outcome.close = true;
        }
        let body = modal::body(area);
        let mut pen = body.top() + 2.0;
        pen = modal::section(ui, body, pen, "General");
        pen = fields(ui, body, pen, &mut form);
        let test = Rect::from_min_size(Pos2::new(body.left(), pen + 10.0), Vec2::new(126.0, 26.0));
        if modal::button(ui, test, "Test Connection", true, false) {
            outcome.test = true;
        }
        // What the server called itself, or why it would not answer - beside the button that asked,
        // so the two read as one thing rather than as an answer floating above the footer.
        if let Some(tested) = &form.tested {
            let (said, tint) = match tested {
                Ok(version) => (version.clone(), color::git_added()),
                Err(why) => (why.clone(), color::unsaved()),
            };
            let painter = ui.painter_at(body);
            let galley = painter.layout(
                said,
                egui::FontId::proportional(11.5),
                tint,
                (body.right() - test.right() - 12.0).max(40.0),
            );
            painter.galley(Pos2::new(test.right() + 12.0, test.top()), galley, tint);
        }
        match modal::footer(ui, area, &[("Cancel", true), ("Save", true)]) {
            Some(0) => outcome.close = true,
            Some(1) => outcome.save = true,
            _ => {}
        }
    });
    if outcome.test {
        form.tested = Some(test_it(&form));
    }
    if outcome.save {
        let mut source = form.source.clone();
        let (secret, said) = secret_of(&form);
        source.secret = secret;
        if let Some(said) = said {
            requests.push(Request::Message(said));
        }
        match explorer.save_source(&form.was, source) {
            Ok(()) => outcome.close = true,
            Err(why) => {
                form.tested = Some(Err(why.clone()));
                requests.push(Request::Message(why));
            }
        }
    }
    let closed = outcome.close || escaped;
    match closed {
        true => explorer.modal = None,
        false => explorer.modal = Some(Modal::Source(form)),
    }
    (requests, closed)
}

/// The fields of the General tab.
fn fields(ui: &mut egui::Ui, body: Rect, top: f32, form: &mut SourceForm) -> f32 {
    let mut pen = top;
    modal::field(ui, labelled(ui, body, pen, "Name"), "Name", &mut form.source.name);
    pen += FIELD + GAP;

    // Two buttons rather than a dropdown, because there are two engines and which one is chosen can
    // then be seen without opening anything — the rule the three line spacings already keep.
    let engines = labelled(ui, body, pen, "Engine");
    for (index, engine) in [Engine::Postgres, Engine::Sqlite].into_iter().enumerate() {
        let button = Rect::from_min_size(
            Pos2::new(engines.left() + index as f32 * 96.0, engines.top()),
            Vec2::new(90.0, engines.height()),
        );
        let name = match engine {
            Engine::Postgres => "PostgreSQL",
            Engine::Sqlite => "SQLite",
        };
        if crate::components::controls::choice_button(ui, button, name, form.source.engine == engine) {
            form.source.engine = engine;
            if engine == Engine::Sqlite {
                form.source.host = String::new();
                form.source.port = 0;
            } else if form.source.port == 0 {
                form.source.host = "localhost".to_owned();
                form.source.port = 5432;
            }
        }
    }
    pen += FIELD + GAP;

    match form.source.engine {
        Engine::Sqlite => {
            // The field and a picker beside it. A path is often quicker pasted or typed than walked
            // to, and an agent cannot press a Browse button at all, so the field is not replaced by
            // the picker - `rfd` is the same dialog `Open File` and `Open Folder` already use.
            let line = labelled(ui, body, pen, "File");
            let browse = Rect::from_min_size(
                Pos2::new(line.right() - 82.0, line.top()),
                Vec2::new(82.0, line.height()),
            );
            let path = Rect::from_min_max(line.min, Pos2::new(browse.left() - 8.0, line.bottom()));
            modal::field(ui, path, "File", &mut form.source.database);
            if modal::button(ui, browse, "Browse\u{2026}", true, false) {
                if let Some(chosen) = choose_a_sqlite_file(&form.source.database) {
                    form.source.database = chosen;
                    // A file that was just picked plainly exists, so whatever the last test said
                    // about the last path no longer applies.
                    form.tested = None;
                }
            }
            pen = modal::note(
                ui,
                body,
                pen + FIELD + 2.0,
                "The path of a SQLite file. It is opened, never created - a mistyped path says so \
                 rather than quietly making an empty database.",
            );
        }
        Engine::Postgres => {
            let line = labelled(ui, body, pen, "Host and port");
            let host = Rect::from_min_size(line.min, Vec2::new(line.width() - 90.0, line.height()));
            let port = Rect::from_min_size(
                Pos2::new(line.right() - 82.0, line.top()),
                Vec2::new(82.0, line.height()),
            );
            modal::field(ui, host, "Host", &mut form.source.host);
            let mut said = form.source.port.to_string();
            if modal::field(ui, port, "Port", &mut said).changed() {
                form.source.port = said.parse().unwrap_or(form.source.port);
            }
            pen += FIELD + GAP;
            modal::field(ui, labelled(ui, body, pen, "Database"), "Database", &mut form.source.database);
            pen += FIELD + GAP;
            modal::field(ui, labelled(ui, body, pen, "User"), "User", &mut form.source.user);
            pen += FIELD + GAP;

            // **The password sits under the user**, which is where `task-1795` asks for it and where
            // every other tool puts it. There is no environment-variable field any more: a password
            // typed here is written to this machine’s own credential store when Save is pressed, and
            // what the settings file records is the *name* of the entry.
            secret_field(ui, labelled(ui, body, pen, "Password"), &mut form.typed);
            pen = modal::note(ui, body, pen + FIELD + 2.0, secret_note(&form.source.secret));

            // **`Connection security`, not `Encryption`.** `task-1795`: "No idea what encryption
            // option means." The row is `sslmode`, the values it stores are unchanged, and the words
            // are now the question a person can answer - with a line saying which of the two things
            // called encryption this one is.
            let modes = labelled(ui, body, pen, "Connection security");
            for (index, mode) in [SslMode::Disable, SslMode::Prefer, SslMode::Require].into_iter().enumerate() {
                let button = Rect::from_min_size(
                    Pos2::new(modes.left() + index as f32 * 92.0, modes.top()),
                    Vec2::new(86.0, modes.height()),
                );
                if crate::components::controls::choice_button_named(
                    ui,
                    button,
                    plain_words_for(mode),
                    &format!("sslmode {}", mode.name()),
                    form.source.ssl == mode,
                ) {
                    form.source.ssl = mode;
                }
            }
            pen = modal::note(
                ui,
                body,
                modes.bottom() + 2.0,
                "Whether the connection to the server is encrypted. Required refuses to connect \
                 without it. This is about the connection, not about how your password is kept - \
                 that is always in this machine’s own credential store.",
            );
        }
    }
    pen
}

/// What each `sslmode` is called on the dialog.
///
/// The stored value is unchanged - `disable`, `prefer` and `require` are what a settings file
/// already holds and what a server is told - so this is a label and nothing more.
fn plain_words_for(mode: SslMode) -> &'static str {
    match mode {
        SslMode::Disable => "Off",
        SslMode::Prefer => "If offered",
        SslMode::Require => "Required",
    }
}

/// Ask for a SQLite file, starting wherever the field already points.
///
/// The same `rfd` dialog `Open File` uses. It blocks inside a frame, which is what a native file
/// dialog is on every platform and what the window already does for `Open File`.
fn choose_a_sqlite_file(said: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Choose a SQLite database")
        .add_filter("SQLite database", &["db", "sqlite", "sqlite3", "db3"])
        .add_filter("Every file", &["*"]);
    // Start where the field points, which on a mistyped path is its folder rather than nowhere.
    let said = std::path::Path::new(said.trim());
    if let Some(folder) = said.parent().filter(|folder| folder.is_dir()) {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = said.file_name().and_then(|name| name.to_str()) {
        dialog = dialog.set_file_name(name);
    }
    dialog.pick_file().map(|path| path.display().to_string())
}

/// What the line under the password field says, which depends on where the password is now.
fn secret_note(secret: &Secret) -> &'static str {
    match secret {
        Secret::Keychain(_) | Secret::Typed(_) => {
            "There is a password kept for this data source in this machine’s own credential store. \
             Type a new one to replace it, or leave this empty to keep the one that is there."
        }
        Secret::Environment(_) => {
            "This data source reads its password from an environment variable, which is how it was \
             set up. Type one here to move it into this machine’s own credential store instead."
        }
        Secret::None => {
            "Kept in this machine’s own credential store - the keychain on macOS, the Secret Service \
             on Linux, Credential Manager on Windows. Unluminate writes no password to a file, ever: \
             what the settings file records is the name of the entry."
        }
    }
}

/// A field that never draws what is in it.
///
/// `egui`'s own password mode, which replaces every character with a dot in the galley rather than in
/// the value — so what is drawn is dots whether or not the field has the keyboard, and a screenshot of
/// this page carries no secret.
fn secret_field(ui: &mut egui::Ui, area: Rect, value: &mut String) {
    ui.painter().rect(
        area,
        egui::CornerRadius::same(crate::theme::size::CONTROL_CORNER),
        color::field(),
        egui::Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
    let id = ui.id().with("database-source-password");
    let text_rect =
        crate::components::controls::field_takes_the_whole_rectangle(ui, area, 8.0, id);
    let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = edit.add(
        egui::TextEdit::singleline(value)
            .id(id)
            .frame(egui::Frame::NONE)
            .password(true)
            .desired_width(text_rect.width())
            .text_color(color::text_control()),
    );
    response.widget_info(|| {
        // Named, never valued: `WidgetInfo::labeled` would otherwise read the field's contents out to
        // a test and to assistive technology.
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Password")
    });
}

/// Where the password for this form is, as the settings file will record it.
///
/// A password typed into the dialog is written to this machine’s own credential store and what
/// comes back is the **name** of the entry — never the value, which is the rule the settings file has
/// always kept. Nothing typed leaves whatever the source already had alone, so opening a data source
/// to change its port does not clear its password.
///
/// A store that refuses says so, and the password is then held in this window only, which is what
/// `Secret::Typed` has always been: refusing to connect at all because a keychain was locked would
/// be a worse answer than a connection that lasts until the window closes.
fn secret_of(form: &SourceForm) -> (Secret, Option<String>) {
    if form.typed.is_empty() {
        return (form.source.secret.clone(), None);
    }
    let entry = crate::services::database::keychain_entry_for(&form.source.name);
    match crate::services::agent_tasks::keychain::write(&entry, &form.typed) {
        Ok(()) => (Secret::Keychain(entry), None),
        Err(why) => (
            Secret::Typed(form.typed.clone()),
            Some(format!(
                "The password is held in this window only: {why}. It is not written to any file."
            )),
        ),
    }
}

/// Open a connection, say what the server called itself, and close it again.
///
/// **The server’s own version string**, rather than a sentence of Unluminate’s about it, which is
/// `unluminate-git`’s rule about quoting a program. Read only for the test whatever the data source is
/// set to: a Test Connection that could write is one nobody should press twice.
fn test_it(form: &SourceForm) -> Result<String, String> {
    let mut source: Source = form.source.clone();
    // What is *typed* rather than what is stored, so Test Connection tests the password on the
    // screen — and without writing anything to the credential store, because a test is not a save.
    if !form.typed.is_empty() {
        source.secret = Secret::Typed(form.typed.clone());
    }
    source.read_only = true;
    let password = crate::services::database::password_for(&source);
    match unluminate_db::Database::connect(&source, password.as_deref()) {
        Ok(mut database) => {
            let said = database.version();
            let encrypted = database.is_encrypted();
            database.close();
            Ok(match encrypted {
                true => format!("{said}, encrypted"),
                false => said,
            })
        }
        Err(why) => Err(why.to_string()),
    }
}


/// How wide the column list is, leaving the rest of the body for the statement beside it.
const COLUMNS: f32 = 400.0;
/// A row of the column list.
const COLUMN_ROW: f32 = 26.0;

/// The New Table dialog: a name, a list of columns, and the statement they compose beside them.
///
/// **The statement on the right is the one that will be sent**, from `unluminate_db::sql::create_table`,
/// which is the same call Create makes — the rule the pending-changes preview already keeps, so what
/// is read and what happens cannot drift apart.
fn new_table(
    explorer: &mut DatabaseExplorer,
    ctx: &egui::Context,
    look: &Look<'_>,
    mut form: TableForm,
) -> (Vec<Request>, bool) {
    let _ = look;
    let mut requests = Vec::new();
    let mut close = false;
    let mut create = false;
    let engine = explorer
        .configuration
        .source(&form.source)
        .map(|source| source.engine)
        .unwrap_or(Engine::Postgres);
    if form.columns.is_empty() {
        form.columns.push(a_first_column(engine));
    }
    let statement = unluminate_db::sql::create_table(&form.schema, &form.name, &form.columns, engine);

    let (_, escaped) = modal::show(ctx, "unluminate-database-new-table", 760.0, 520.0, |ui, area| {
        if modal::header(ui, area, "New Table") {
            close = true;
        }
        let body = modal::body(area);
        let mut pen = body.top() + 2.0;

        // Which data source and schema, said rather than chosen: the dialog is opened from a row of
        // the tree, and a chooser here would be a second way to say a thing the click already said.
        let where_it_goes = format!("{} · {}", form.source, match form.schema.is_empty() {
            true => "no schema".to_owned(),
            false => form.schema.clone(),
        });
        modal::label(ui.painter(), Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), FIELD)), body.left(), &where_it_goes, color::text_faint(), 11.5);
        pen += FIELD;

        modal::field(ui, labelled(ui, body, pen, "Name"), "Table name", &mut form.name);
        pen += FIELD + GAP * 2.0;

        let left = Rect::from_min_max(Pos2::new(body.left(), pen), Pos2::new(body.left() + COLUMNS, body.bottom() - 8.0));
        let right = Rect::from_min_max(Pos2::new(left.right() + 16.0, pen), Pos2::new(body.right(), body.bottom() - 8.0));
        let after = modal::section(ui, left, pen, "Columns");
        columns(ui, left, after, &mut form, engine);
        let after_right = modal::section(ui, right, pen, "SQL");
        let block = Rect::from_min_max(Pos2::new(right.left(), after_right), right.max);
        let said = match (&statement, &form.problem) {
            (_, Some(problem)) => problem.clone(),
            (Ok(sql), None) => format!("{sql};"),
            (Err(why), None) => why.clone(),
        };
        modal::monospaced(ui, block, "database-new-table-sql", &said);

        match modal::footer(ui, area, &[("Cancel", true), ("Create", statement.is_ok())]) {
            Some(0) => close = true,
            Some(1) => create = true,
            _ => {}
        }
    });

    if create {
        match statement {
            Ok(sql) => match explorer.run_the_ddl(&form.source, &form.schema, &sql) {
                Ok(said) => {
                    requests.push(Request::Message(said));
                    close = true;
                }
                Err(why) => form.problem = Some(why),
            },
            Err(why) => form.problem = Some(why),
        }
    }
    let closed = close || escaped;
    match closed {
        true => explorer.modal = None,
        false => explorer.modal = Some(Modal::NewTable(form)),
    }
    (requests, closed)
}

/// The column a new table starts with: a key called `id`, of whichever integer type counts itself up.
fn a_first_column(engine: Engine) -> ColumnForm {
    ColumnForm {
        name: "id".to_owned(),
        type_name: match engine {
            Engine::Sqlite => "INTEGER".to_owned(),
            Engine::Postgres => "integer".to_owned(),
        },
        in_key: true,
        not_null: true,
    }
}

/// The list of columns: a name, a type, `PK`, `NN`, and a cross to take the row away.
fn columns(ui: &mut egui::Ui, area: Rect, top: f32, form: &mut TableForm, engine: Engine) {
    let mut pen = top;
    let mut removing: Option<usize> = None;
    // What is left after the type field, the two tick boxes and the cross. The tick boxes need 46
    // apiece rather than 34: `modal::check` draws its word ten points past a sixteen point box, so
    // at 34 the `PK` label was drawn under the `NN` box beside it. `task-1795`.
    let name_width = area.width() - 286.0;
    for index in 0..form.columns.len() {
        let row = Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), COLUMN_ROW));
        let name = Rect::from_min_size(row.min, Vec2::new(name_width.max(90.0), FIELD));
        modal::field(ui, name, &format!("Column {} name", index + 1), &mut form.columns[index].name);

        let kind = Rect::from_min_size(Pos2::new(name.right() + 6.0, row.top()), Vec2::new(140.0, FIELD));
        // A dropdown that also takes typing: the field is the value and the chevron beside it offers
        // the engine’s own list. See `Engine::column_types` for why the list is not closed.
        let typed = Rect::from_min_max(kind.min, Pos2::new(kind.right() - 22.0, kind.bottom()));
        modal::field(ui, typed, &format!("Column {} type", index + 1), &mut form.columns[index].type_name);
        let chevron = Rect::from_min_size(Pos2::new(kind.right() - 22.0, kind.top()), Vec2::new(22.0, FIELD));
        let chosen = crate::components::controls::dropdown(
            ui,
            chevron,
            "",
            &format!("Column {} type list", index + 1),
            None,
            |ui| {
                let mut chosen = None;
                for offered in engine.column_types() {
                    if ui.selectable_label(false, *offered).clicked() {
                        chosen = Some((*offered).to_owned());
                    }
                }
                chosen
            },
        );
        if let Some(chosen) = chosen {
            form.columns[index].type_name = chosen;
        }

        // Named per column and drawn as two letters: every row draws the same pair, so a name of its
        // own is what keeps two controls from sharing one — the rule the grid's cells already keep by
        // being `title row 1` — and it is what gives each its own id.
        let key = Rect::from_min_size(Pos2::new(kind.right() + 8.0, row.top() + 2.0), Vec2::new(46.0, 20.0));
        modal::check_named(ui, key, "PK", &format!("Column {} PK", index + 1), &mut form.columns[index].in_key);
        let not_null = Rect::from_min_size(Pos2::new(key.right() + 8.0, row.top() + 2.0), Vec2::new(46.0, 20.0));
        modal::check_named(ui, not_null, "NN", &format!("Column {} NN", index + 1), &mut form.columns[index].not_null);

        // Absent on the only row there is: a control that cannot apply is not drawn, and a table with
        // no columns at all is not a table.
        if form.columns.len() > 1 {
            let cross = Rect::from_center_size(Pos2::new(row.right() - 12.0, row.center().y), Vec2::splat(18.0));
            if crate::components::controls::icon_button(
                ui,
                cross,
                &format!("Remove column {}", index + 1),
                crate::theme::icon::cross,
            ) {
                removing = Some(index);
            }
        }
        pen = row.bottom() + 4.0;
    }
    if let Some(index) = removing {
        form.columns.remove(index);
    }
    let add = Rect::from_min_size(Pos2::new(area.left(), pen + 4.0), Vec2::new(120.0, 26.0));
    if modal::button(ui, add, "Add column", true, false) {
        form.columns.push(ColumnForm {
            type_name: engine.column_types().first().map(|first| (*first).to_owned()).unwrap_or_default(),
            ..ColumnForm::default()
        });
    }
}

/// The statements Submit will send.
fn preview(explorer: &mut DatabaseExplorer, ctx: &egui::Context, page: u64) -> (Vec<Request>, bool) {
    let text = match explorer.preview(page) {
        Ok(statements) if statements.is_empty() => "There is nothing pending.".to_owned(),
        Ok(statements) => statements
            .iter()
            .map(|statement| {
                let values: Vec<String> = statement
                    .values
                    .iter()
                    .enumerate()
                    .map(|(at, value)| match value.is_null() {
                        true => format!("  ${} = NULL", at + 1),
                        false => format!("  ${} = {}", at + 1, value.display()),
                    })
                    .collect();
                format!("-- {}\n{};\n{}", statement.what, statement.sql, values.join("\n"))
            })
            .collect::<Vec<String>>()
            .join("\n\n"),
        Err(why) => why,
    };
    reading(explorer, ctx, "Pending changes", &text)
}

/// A modal that shows a block of text and nothing else.
fn reading(
    explorer: &mut DatabaseExplorer,
    ctx: &egui::Context,
    heading: &str,
    text: &str,
) -> (Vec<Request>, bool) {
    let mut requests = Vec::new();
    let mut close = false;
    let mut copy = false;
    let (_, escaped) = modal::show(ctx, "unluminate-database-reading", 640.0, 470.0, |ui, area| {
        if modal::header(ui, area, heading) {
            close = true;
        }
        let body = modal::body(area);
        modal::monospaced(ui, body, "database-reading-text", text);
        match modal::footer(ui, area, &[("Copy", true), ("Done", true)]) {
            Some(0) => copy = true,
            Some(1) => close = true,
            _ => {}
        }
    });
    if copy {
        // The window owns the one handle to the clipboard, so a provider asks rather than reaching —
        // `Request::Copy`'s own reason.
        requests.push(Request::Copy(text.to_owned()));
    }
    let closed = close || escaped;
    if closed {
        explorer.modal = None;
    }
    (requests, closed)
}

