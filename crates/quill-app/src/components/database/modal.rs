//! The plugin's four modals, all built from `components::modal`'s own furniture.
//!
//! A tenth modal that drew its own header would be a tenth modal that almost agrees with the other
//! nine, so the frame, the header, the body, the footer and the buttons are the window's — and so are
//! the dragging and the resizing, which `modal::show` gives every dialog without asking.
//!
//! - **New Data Source** — IntelliJ's General tab, cut to what applies: name, engine, host, port,
//!   database, user, where the password is, ssl mode, read only, and Test Connection reporting the
//!   server's own version string.
//! - **Preview pending changes** — the actual statements Submit will send, from the same call Submit
//!   makes, so the preview cannot drift from what happens.
//! - **DDL** — a `CREATE` statement.
//! - **Confirm** — a statement from a console that changes rows.

use egui::{Pos2, Rect, Vec2};

use quill_db::source::{Engine, Secret, Source, SslMode};

use crate::components::modal;
use crate::services::database::{DatabaseExplorer, Modal, SourceForm};
use crate::services::plugin_ui::{Look, Request};
use crate::theme::color;

/// How wide the label column is, so every field on the dialog starts at the same x.
///
/// A label column rather than a name inside each field, which is IntelliJ's own arrangement and the
/// one thing the first version of this dialog got wrong: five unlabelled boxes told a person nothing
/// about which was the database and which was the user.
const LABEL: f32 = 112.0;
/// A field's own height, and the gap under a row.
const FIELD: f32 = 24.0;
const GAP: f32 = 6.0;

/// A label at the left, and the rectangle the field goes in, which starts past it.
fn labelled(ui: &egui::Ui, body: Rect, pen: f32, name: &str) -> Rect {
    let row = Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), FIELD));
    crate::components::modal::label(ui.painter(), row, row.left(), name, color::TEXT_DIM, 12.0);
    Rect::from_min_max(Pos2::new(body.left() + LABEL, pen), Pos2::new(body.right(), pen + FIELD))
}

/// Draw whichever modal is open. Answers what it asked for and whether it closed.
pub fn show(explorer: &mut DatabaseExplorer, ctx: &egui::Context, look: &Look<'_>) -> (Vec<Request>, bool) {
    let Some(open) = explorer.modal.clone() else { return (Vec::new(), false) };
    match open {
        Modal::Source(form) => source_modal(explorer, ctx, look, form),
        Modal::Preview { page } => preview(explorer, ctx, page),
        Modal::Ddl { title, text } => reading(explorer, ctx, &format!("DDL — {title}"), &text),
        Modal::Confirm { page, statement } => confirm(explorer, ctx, page, &statement),
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
    // **The heights add up**, which is the whole of a modal's difficulty here: four sections are laid
    // out one under another with no scroll, so a budget that overflows draws the last section over the
    // buttons — which is exactly what the first version of this dialog did. 560 is what the rows below
    // need, counted rather than guessed.
    let (_, escaped) = modal::show(ctx, "quill-database-source", 480.0, 560.0, |ui, area| {
        if modal::header(ui, area, heading) {
            outcome.close = true;
        }
        let body = modal::body(area);
        let mut pen = body.top() + 2.0;
        pen = modal::section(ui, body, pen, "General");
        pen = fields(ui, body, pen, &mut form);
        pen = modal::section(ui, body, pen, "Password");
        pen = password(ui, body, pen, &mut form);
        pen = modal::section(ui, body, pen, "Safety");
        let row = Rect::from_min_size(Pos2::new(body.left(), pen), Vec2::new(body.width(), 22.0));
        modal::check(ui, row, "Read only", &mut form.source.read_only);
        pen = modal::note(
            ui,
            body,
            row.bottom() + 4.0,
            "Enforced by the server — a read-only PostgreSQL session, and SQLite opened read only — \
             rather than by a check in Quill. The editing controls are hidden as well.",
        );
        let test = Rect::from_min_size(Pos2::new(body.left(), pen + 10.0), Vec2::new(126.0, 26.0));
        if modal::button(ui, test, "Test Connection", true, false) {
            outcome.test = true;
        }
        // What the server called itself, or why it would not answer - beside the button that asked,
        // so the two read as one thing rather than as an answer floating above the footer.
        if let Some(tested) = &form.tested {
            let (said, tint) = match tested {
                Ok(version) => (version.clone(), color::GIT_ADDED),
                Err(why) => (why.clone(), color::UNSAVED),
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
        source.secret = secret_of(&form);
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
            modal::field(ui, labelled(ui, body, pen, "File"), "File", &mut form.source.database);
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
            let modes = labelled(ui, body, pen, "Encryption");
            for (index, mode) in [SslMode::Disable, SslMode::Prefer, SslMode::Require].into_iter().enumerate() {
                let button = Rect::from_min_size(
                    Pos2::new(modes.left() + index as f32 * 80.0, modes.top()),
                    Vec2::new(74.0, modes.height()),
                );
                if crate::components::controls::choice_button_named(
                    ui,
                    button,
                    mode.name(),
                    &format!("sslmode {}", mode.name()),
                    form.source.ssl == mode,
                ) {
                    form.source.ssl = mode;
                }
            }
            pen += FIELD + GAP;
        }
    }
    pen
}

/// Where the password is - never what it is.
fn password(ui: &mut egui::Ui, body: Rect, top: f32, form: &mut SourceForm) -> f32 {
    if form.source.engine == Engine::Sqlite {
        return modal::note(ui, body, top, "A SQLite database is a file, and has no password.");
    }
    let variable = labelled(ui, body, top, "In the variable");
    modal::field(ui, variable, "Environment variable", &mut form.variable);
    let pen = modal::note(
        ui,
        body,
        variable.bottom() + 2.0,
        "The NAME of an environment variable holding the password, read when a connection is opened \
         and never held. Quill writes no password to disk, ever.",
    );
    let typed = labelled(ui, body, pen, "Or type one");
    secret_field(ui, typed, &mut form.typed);
    modal::note(
        ui,
        body,
        typed.bottom() + 2.0,
        "Held in this window and written nowhere: where IntelliJ offers Save: Forever, this offers \
         until this window closes.",
    )
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
        color::FIELD,
        egui::Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
    let text_rect = crate::components::controls::field_text_rect(ui, area, 8.0);
    let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = edit.add(
        egui::TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .password(true)
            .desired_width(text_rect.width())
            .text_color(color::TEXT_CONTROL),
    );
    response.widget_info(|| {
        // Named, never valued: `WidgetInfo::labeled` would otherwise read the field's contents out to
        // a test and to assistive technology.
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Password for this window")
    });
}

/// What was typed, as a place a password is.
fn secret_of(form: &SourceForm) -> Secret {
    if !form.variable.trim().is_empty() {
        return Secret::Environment(form.variable.trim().to_owned());
    }
    if !form.typed.is_empty() {
        return Secret::Typed(form.typed.clone());
    }
    Secret::None
}

/// Open a connection, say what the server called itself, and close it again.
///
/// **The server's own version string**, which is what IntelliJ's Test Connection reports and what
/// `quill-git`'s rule about quoting a program asks for. Read only for the test whatever the data
/// source is set to: a Test Connection that could write is one nobody should press twice.
fn test_it(form: &SourceForm) -> Result<String, String> {
    let mut source: Source = form.source.clone();
    source.secret = secret_of(form);
    source.read_only = true;
    let password = crate::services::database::password_for(&source);
    match quill_db::Database::connect(&source, password.as_deref()) {
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
    let (_, escaped) = modal::show(ctx, "quill-database-reading", 640.0, 470.0, |ui, area| {
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

/// A statement from a console that changes rows.
fn confirm(
    explorer: &mut DatabaseExplorer,
    ctx: &egui::Context,
    page: u64,
    statement: &str,
) -> (Vec<Request>, bool) {
    let mut send = false;
    let mut close = false;
    let (_, escaped) = modal::show(ctx, "quill-database-confirm", 560.0, 260.0, |ui, area| {
        if modal::header(ui, area, "Run this statement?") {
            close = true;
        }
        let body = modal::body(area);
        let block = Rect::from_min_max(body.min, Pos2::new(body.right(), body.bottom() - 52.0));
        modal::monospaced(ui, block, "database-confirm-text", statement);
        modal::note(
            ui,
            body,
            block.bottom() + 6.0,
            "This statement changes rows. Turn the confirmation off in Settings, Database if you \
             would rather it did not ask.",
        );
        match modal::footer(ui, area, &[("Cancel", true), ("Run it", true)]) {
            Some(0) => close = true,
            Some(1) => {
                send = true;
                close = true;
            }
            _ => {}
        }
    });
    let closed = close || escaped;
    if closed {
        explorer.modal = None;
    }
    if send {
        explorer.send_from_console(page, statement);
    }
    (Vec::new(), closed)
}
