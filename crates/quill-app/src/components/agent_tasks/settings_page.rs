//! The Agent-Tasks page in the Settings window.
//!
//! The Settings window is one size for every page and a page does not scroll, so this page is written to fit
//! the 640 points every page gets rather than to grow. It is made of the same rows and fields
//! `components::modal` gives every other page, so it is the same furniture as the other five.
//!
//! ## Every value can be copied and every setting can be changed
//!
//! A page that showed a path and gave no way to take it was a page somebody retyped a path out of. Each row
//! has a `Copy` button, which is what `Settings -> Tools -> MCP` already does with the configuration an agent
//! is handed.
//!
//! ## No secret is drawn and no secret is written to a file
//!
//! The authentication key row says `set` or `not set` and never the key, because a page that showed it would be
//! a page somebody screenshots. What is typed into it goes to the machine's own keychain through
//! `services::agent_tasks::keychain`, and the settings file holds the **name** of that entry rather than what
//! is in it.

use egui::{CornerRadius, Pos2, Rect, Vec2};

use super::text;
use crate::services::agent_tasks::model::Assignee;
use crate::services::agent_tasks::{AgentTasks, EFFORTS};
use crate::services::plugin_ui::{Look, Request};

const PAD: f32 = 12.0;
/// How tall one setting is: its name, its value and the line that says what it does.
const ROW: f32 = 58.0;
/// How wide the value part of a row is, leaving room for the buttons on its right.
const VALUE: f32 = 300.0;

/// Draw the page inside the rectangle every page gets.
pub fn show(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    // **Scrolled.** The Settings window is one size for every page and a page does not scroll, which was the rule
    // until a page had more rows than 640 points holds: this one's last three — the key, its two buttons — were
    // below the footer and could not be reached at all. `tasks/ui-plugin-architecture.md` §4.4 named this as the
    // moment to build a scrolling page area rather than to make the window taller for the sixth time, and this is
    // it. The other five pages are unchanged and still do not scroll.
    //
    // `task-28`: **the bar moved and the page did not.** The rectangle handed to `rows` was read here, outside
    // the scrolling area, and every row is painted at an absolute position measured from it. Scrolling moves the
    // `Ui` inside the area; nothing was reading that `Ui`. So `rows` reads its own rectangle now, from the `Ui` it
    // is given, and says how tall it drew so the bar describes the page rather than describing whichever widget
    // happened to allocate the lowest rectangle.
    let height = ui.available_rect_before_wrap().height();
    let mut requests = Vec::new();
    let scrolled = egui::ScrollArea::vertical()
        .id_salt("agent-tasks-settings")
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| rows(board, ui, look));
    requests.extend(scrolled.inner);
    requests
}

/// The rows themselves, laid out one under another inside the scrolling area.
///
/// The rectangle comes from the `Ui` this is given rather than from the caller, because inside a scrolling area
/// that `Ui`'s origin is where the scroll offset has put it. See [`show`].
fn rows(board: &mut AgentTasks, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let mut requests = Vec::new();
    let mut pen = area.min.y + PAD;
    let configuration = board.configuration().clone();
    let mut changed = configuration.clone();

    heading(ui, look, area, &mut pen, "The board");

    // The board file. Read only here, because moving a database is a thing to do to a file rather than a thing
    // to type: what this offers is the path, so it can be pasted into a backup script or a terminal.
    let where_it_is = board.board_where();
    let outcome = row(ui, look, area, &mut pen, Row {
        name: "Board file",
        value: &where_it_is,
        explanation: "One SQLite file. Copy the path to back it up, `Reveal` to open the folder it is in, or set \
                      `database` in this plugin's own settings.conf to move it.",
        monospace: true,
        copy: true,
        reveal: true,
    });
    outcome.act(&mut requests, &where_it_is);
    if outcome.revealed {
        requests.push(Request::Reveal(configuration.database_path()));
    }

    let project = configuration.project.clone().map(|path| path.display().to_string()).unwrap_or_default();
    let typed = field(ui, look, area, &mut pen, Field {
        name: "Default project",
        value: &project,
        hint: "the folder this window has open",
        explanation: "Where an agent is launched when a ticket names no project of its own.",
    });
    if let Some(typed) = typed {
        changed.project = Some(typed)
            .filter(|value: &String| !value.trim().is_empty())
            .map(std::path::PathBuf::from);
    }

    let lease = configuration.lease_minutes.to_string();
    let typed = field(ui, look, area, &mut pen, Field {
        name: "Lease",
        value: &lease,
        hint: "45",
        explanation: "How many minutes the board waits to hear from an agent before the watchdog calls its lease \
                      expired and starts nudging it.",
    });
    if let Some(typed) = typed {
        if let Ok(minutes) = typed.trim().parse::<i64>() {
            if minutes > 0 {
                changed.lease_minutes = minutes;
            }
        }
    }

    heading(ui, look, area, &mut pen, "What a new ticket gets");

    // The agent, as a row of choices rather than a dropdown: there are two, and two buttons are quicker to
    // read than a list that has to be opened. `components::controls` has no dropdown that takes a borrowed
    // list of two.
    let agent_row = Rect::from_min_size(Pos2::new(area.min.x + PAD, pen), Vec2::new(area.width() - PAD * 2.0, ROW));
    let painter = ui.painter().clone();
    named(ui, &painter, agent_row.min, "Default agent", look);
    let mut left = agent_row.min.x;
    for agent in [Assignee::Claude, Assignee::Codex] {
        let at = Rect::from_min_size(
            Pos2::new(left, agent_row.min.y + look.font_size + 4.0),
            Vec2::new(84.0, 24.0),
        );
        if crate::components::controls::choice_button(ui, at, agent.name(), configuration.agent == agent) {
            changed.agent = agent;
        }
        left += 90.0;
    }
    pen = explain(
        &painter,
        look,
        agent_row,
        agent_row.min.y + look.font_size + 32.0,
        "Which agent a new ticket is assigned to, and which one takes a ticket assigned to a person.",
    );

    // A dropdown rather than a field, and the **same** control a ticket's own `Model` is, so the value a new
    // ticket gets and the value a ticket holds are chosen from one list. `task-28`.
    let model = configuration.model.clone().unwrap_or_default();
    let models: Vec<(String, String)> =
        crate::services::agent_tasks::agent::models_for(configuration.agent, configuration.model.as_deref())
            .into_iter()
            .map(|name| (name.clone(), name))
            .collect();
    let picked = choice(ui, look, area, &mut pen, Choice {
        name: "Default model",
        options: &models,
        chosen: &model,
        empty: "the agent's own default",
        explanation: "Passed as `--model`. The models the chosen agent has, and whatever this setting already \
                      names if it is not one of them.",
    });
    if let Some(picked) = picked {
        changed.model = Some(picked).filter(|value: &String| !value.trim().is_empty());
    }

    let efforts: Vec<(String, String)> =
        EFFORTS.iter().map(|level| ((*level).to_owned(), (*level).to_owned())).collect();
    let picked = choice(ui, look, area, &mut pen, Choice {
        name: "Default effort",
        options: &efforts,
        chosen: configuration.effort.as_deref().unwrap_or(""),
        empty: "the agent's own default",
        explanation: "Passed as `--effort`. Codex knows nothing above `high`, so `xhigh` and `max` collapse onto \
                      it.",
    });
    if let Some(picked) = picked {
        changed.effort = Some(picked).filter(|value: &String| !value.trim().is_empty());
    }

    heading(ui, look, area, &mut pen, "How an agent is launched");

    // **A whole command rather than an extra-flags field**, so the program itself can be named: a
    // wrapper script, a particular version under a version manager, or `claude` by its full path.
    // Split the way a run configuration is split, so **no shell runs it** and what is typed is what
    // runs. Empty means the command Quill builds itself, which is what these hints say.
    let claude = configuration.claude_command.clone().unwrap_or_default();
    let typed = field(ui, look, area, &mut pen, Field {
        name: "Claude command",
        value: &claude,
        hint: "claude --dangerously-skip-permissions",
        explanation: "The program and the flags in front of it, for a ticket assigned to Claude. The ticket's own \
                      model and effort and the session flag are added after it, because those come from the row \
                      and the board has to be able to resume the conversation it named. No shell runs this, so \
                      nothing expands and a path with spaces in it goes in \"quotes\".",
    });
    if let Some(typed) = typed {
        changed.claude_command = Some(typed).filter(|value: &String| !value.trim().is_empty());
    }

    let codex = configuration.codex_command.clone().unwrap_or_default();
    let typed = field(ui, look, area, &mut pen, Field {
        name: "Codex command",
        value: &codex,
        hint: "codex",
        explanation: "The same for a ticket assigned to Codex. `resume <id>` stays the first thing after the \
                      program, because it is a subcommand rather than a flag.",
    });
    if let Some(typed) = typed {
        changed.codex_command = Some(typed).filter(|value: &String| !value.trim().is_empty());
    }

    heading(ui, look, area, &mut pen, "Where the agent connects");

    // **Two settings, not four.** `task-28`: this asked for a Base URL, a Key name naming a keychain entry, a
    // Key variable naming an environment variable, and then the key — three of which are Quill's own plumbing
    // described to the person using it. What a connection is, is a gateway and a key.
    let base = configuration.base_url.clone().unwrap_or_default();
    let typed = field(ui, look, area, &mut pen, Field {
        name: "Iliad URL",
        value: &base,
        hint: "the agent's own endpoint",
        explanation: "The gateway the agent talks to, handed to it as ANTHROPIC_BASE_URL and OPENAI_BASE_URL. \
                      Empty means whichever endpoint the agent itself is configured for, which is \
                      api.anthropic.com for Claude.",
    });
    if let Some(typed) = typed {
        changed.base_url = Some(typed).filter(|value: &String| !value.trim().is_empty());
    }

    // The key itself. Never drawn, and what is typed goes straight to the keychain rather than into the
    // configuration, so it is not in this value, not in the settings file and not in a screenshot.
    // **Read once**, not once a frame. `keychain::is_set` runs the platform's own tool and copies the secret into
    // this process to answer, so asking it 60 times a second spawned 60 processes a second and could provoke a
    // keychain prompt on every one of them. `AgentTasks` remembers the answer and forgets it when the name changes
    // or a key is saved.
    let found = board.where_the_key_is();
    // Whether this machine has a keychain Quill can drive at all. macOS has `security` and Linux has
    // `secret-tool`; Windows has neither written yet, and a page that offered to save a key there would be
    // offering something that cannot happen.
    let has_a_keychain = cfg!(any(target_os = "macos", target_os = "linux"));
    // **Where it came from, not just whether there is one.** A key can reach the agent from two places, and a
    // page that said `set` without saying which would leave somebody wondering why clearing the keychain
    // changed nothing.
    let key_state = match (found, has_a_keychain) {
        (Some(source), _) => format!("set, from {source}"),
        (None, true) => "not set: type it here, or export ANTHROPIC_API_KEY in your shell profile".to_owned(),
        (None, false) => format!(
            "not set, and Quill has no keychain on {}: export ANTHROPIC_API_KEY in your shell profile",
            std::env::consts::OS
        ),
    };
    let secret_row = Rect::from_min_size(Pos2::new(area.min.x + PAD, pen), Vec2::new(area.width() - PAD * 2.0, ROW));
    let painter = ui.painter().clone();
    named(ui, &painter, secret_row.min, "Iliad key", look);
    let at = Rect::from_min_size(
        Pos2::new(secret_row.min.x, secret_row.min.y + look.font_size + 4.0),
        Vec2::new(VALUE, 24.0),
    );
    ui.painter().rect(
        at,
        CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    // **Absent rather than dimmed** on a platform with no keychain, which is the rule the start button on a card
    // and the `F` button already follow: the field and its two buttons cannot do anything there, so what is drawn
    // in their place is the sentence saying why.
    if !has_a_keychain {
        painter.text(
            Pos2::new(at.min.x + 2.0, at.center().y),
            egui::Align2::LEFT_CENTER,
            &key_state,
            egui::FontId::proportional(look.font_size - 1.0),
            look.palette.text_faint,
        );
    }
    let mut secret = board.key_draft.clone();
    if has_a_keychain {
        let response = ui.put(
            crate::components::controls::field_text_rect(ui, at, 8.0),
            egui::TextEdit::singleline(&mut secret)
                .frame(egui::Frame::NONE)
                .password(true)
                .hint_text(egui::RichText::new(&key_state).color(look.palette.text_faint))
                .font(egui::FontId::proportional(look.font_size - 0.5))
                .text_color(look.palette.text),
        );
        if response.changed() {
            board.key_draft = secret;
        }
        let save = Rect::from_min_size(Pos2::new(at.max.x + 8.0, at.min.y), Vec2::new(70.0, 24.0));
        let clear = Rect::from_min_size(Pos2::new(save.max.x + 6.0, at.min.y), Vec2::new(60.0, 24.0));
        if crate::components::controls::choice_button(ui, save, "Save key", false) {
            match board.save_the_key() {
                Ok(said) => requests.push(Request::Message(said)),
                Err(problem) => requests.push(Request::Message(problem)),
            }
        }
        if crate::components::controls::choice_button(ui, clear, "Clear", false) {
            board.key_draft.clear();
            match board.clear_the_key() {
                Ok(said) => requests.push(Request::Message(said)),
                Err(problem) => requests.push(Request::Message(problem)),
            }
        }
    }
    // The last row, so what it answers is how tall the page is — which the scrolling area reads.
    let bottom = explain(
        &painter,
        look,
        secret_row,
        secret_row.min.y + look.font_size + 32.0,
        match has_a_keychain {
            true =>
                "Typed once and never shown again. It goes to this machine's keychain under `iliad`, not to a \
                 file, and Quill reads it at the moment an agent is launched and hands it over as \
                 ANTHROPIC_API_KEY, OPENAI_API_KEY and ILIAD_API_KEY. Nothing needs typing here when your shell \
                 profile already exports ANTHROPIC_API_KEY: Quill reads that profile whether it was started from \
                 a terminal or from the Dock, so an agent gets what a command typed in the terminal tile gets. \
                 The agent's own environment is where it ends up, which any program running as you can read, so \
                 treat it as a key that machine holds.",
            false =>
                "There is no keychain here for Quill to write to, so nothing can be typed. Export \
                 ANTHROPIC_API_KEY in your shell profile and the agent is given it.",
        },
    );

    // **The height the page actually drew.** `task-28`: the scroll bar's length came from whichever widget
    // happened to allocate the lowest rectangle, which was the key field, and the rows themselves allocate
    // nothing because they are painted at absolute positions. So the bar described nothing in particular. This
    // is the one call that makes it describe the page.
    let drew = (bottom + PAD - area.min.y).max(0.0);
    ui.allocate_space(Vec2::new(area.width(), drew));

    // Written once anything above changed, on the same terms the window's own settings are: once the pointer
    // is up rather than on every frame of a drag.
    if changed != configuration {
        if let Err(problem) = board.change_the_configuration(changed) {
            requests.push(Request::Message(problem));
        }
    }
    requests
}

/// A heading over a group of settings, which is what `Settings -> Appearance` puts over its two.
fn heading(ui: &mut egui::Ui, look: &Look<'_>, area: Rect, pen: &mut f32, said: &str) {
    let at = Pos2::new(area.min.x + PAD, *pen);
    let width = text(ui.painter(), at, said, look.font_size + 1.0, look.palette.text_strong);
    // Named, because every control in Quill has a plain name and a test finds one by it. A heading over a group
    // of settings is what says which group a field belongs to.
    let response = ui.interact(
        Rect::from_min_size(at, Vec2::new(width, look.font_size + 2.0)),
        ui.id().with(("agent-tasks-heading", said)),
        egui::Sense::hover(),
    );
    let name = said.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name.clone()));
    *pen += look.font_size + 12.0;
}

/// One setting that is read rather than typed.
struct Row<'a> {
    name: &'a str,
    value: &'a str,
    explanation: &'a str,
    /// True for a path or an identifier, which reads better in the terminal's font.
    monospace: bool,
    copy: bool,
    /// True for a value that is a path, which `Reveal` opens the folder of.
    reveal: bool,
}

/// What a read only row reported.
#[derive(Clone, Copy)]
struct RowOutcome {
    copied: bool,
    revealed: bool,
}

impl RowOutcome {
    fn act(self, requests: &mut Vec<Request>, value: &str) {
        if self.copied {
            requests.push(Request::Copy(value.to_owned()));
            requests.push(Request::Message("copied".to_owned()));
        }
    }
}

fn row(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pen: &mut f32,
    settings_row: Row<'_>,
) -> RowOutcome {
    let painter = ui.painter().clone();
    let at = Rect::from_min_size(Pos2::new(area.min.x + PAD, *pen), Vec2::new(area.width() - PAD * 2.0, ROW));
    named(ui, &painter, at.min, settings_row.name, look);
    let font = match settings_row.monospace {
        true => egui::FontId::monospace(look.font_size - 1.5),
        false => egui::FontId::proportional(look.font_size - 0.5),
    };
    // **Measured, not assumed.** A value that wrapped to two lines and an explanation that wrapped to three used
    // to be drawn over the next setting's name, because every row advanced by the same 58 points whatever it drew.
    let value = painter.layout(settings_row.value.to_owned(), font, look.palette.text_control, VALUE);
    let value_height = value.size().y;
    let value_top = at.min.y + look.font_size + 6.0;
    painter.galley(Pos2::new(at.min.x, value_top), value, look.palette.text_control);
    let mut copied = false;
    let mut revealed = false;
    let mut button_x = at.min.x + VALUE + 8.0;
    if settings_row.copy {
        let button = Rect::from_min_size(Pos2::new(button_x, value_top), Vec2::new(54.0, 22.0));
        // Inside this row's own id scope. `controls::choice_button` builds its id from the word on it, and every
        // one of these says `Copy`: without a scope they were one widget drawn five times, which egui reports as a
        // red warning and which makes the wrong button respond to a press.
        copied = ui
            .push_id(settings_row.name, |ui| {
                crate::components::controls::choice_button(ui, button, "Copy", false)
            })
            .inner;
        button_x += 60.0;
    }
    if settings_row.reveal {
        let button = Rect::from_min_size(Pos2::new(button_x, value_top), Vec2::new(64.0, 22.0));
        revealed = ui
            .push_id((settings_row.name, "reveal"), |ui| {
                crate::components::controls::choice_button(ui, button, "Reveal", false)
            })
            .inner;
    }
    *pen = explain(&painter, look, at, value_top + value_height.max(22.0) + 4.0, settings_row.explanation);
    RowOutcome { copied, revealed }
}

/// The line under a setting saying what it does, wrapped to the row's width, answering where the next row starts.
///
/// Wrapped, because an explanation drawn with `layout_no_wrap` ran off the right edge of the page and out of the
/// window, and measured, because a row that advanced by a fixed height drew over the setting under it.
fn explain(
    painter: &egui::Painter,
    look: &Look<'_>,
    at: Rect,
    top: f32,
    said: &str,
) -> f32 {
    let galley = painter.layout(
        said.to_owned(),
        egui::FontId::proportional(look.font_size - 2.0),
        look.palette.text_faint,
        at.width(),
    );
    let height = galley.size().y;
    painter.galley(Pos2::new(at.min.x, top), galley, look.palette.text_faint);
    top + height + 14.0
}

/// One setting that is typed.
/// One setting chosen from a list.
struct Choice<'a> {
    name: &'a str,
    /// The `(value, said)` pairs the list offers.
    options: &'a [(String, String)],
    chosen: &'a str,
    /// What the list calls holding nothing, since every setting on this page may be left unset.
    empty: &'a str,
    explanation: &'a str,
}

/// Draw a setting that is chosen from a list, and answer what was chosen when it changed.
///
/// The control itself is `super::value_dropdown`, which is what a ticket's own fields are drawn with, so
/// `Default model` here and `Model` on a ticket are the same list drawn the same way. What is different is the
/// furniture around it — this page puts a strong name above each setting and a sentence under it — which is why
/// there is a wrapper here rather than a second dropdown.
fn choice(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pen: &mut f32,
    settings_choice: Choice<'_>,
) -> Option<String> {
    let painter = ui.painter().clone();
    let at = Rect::from_min_size(Pos2::new(area.min.x + PAD, *pen), Vec2::new(area.width() - PAD * 2.0, ROW));
    // Painted rather than named, for the reason `ticket_modal::dropdown_row` paints its own: the dropdown below
    // answers to this name, and two nodes sharing one name is a name a test cannot ask for.
    text(&painter, at.min, settings_choice.name, look.font_size, look.palette.text_strong);
    let box_at = Rect::from_min_size(
        Pos2::new(at.min.x, at.min.y + look.font_size + 6.0),
        Vec2::new(VALUE, 24.0),
    );
    let picked = super::value_dropdown(
        ui,
        box_at,
        settings_choice.name,
        settings_choice.options,
        settings_choice.chosen,
        Some(settings_choice.empty),
    );
    *pen = explain(&painter, look, at, box_at.max.y + 6.0, settings_choice.explanation);
    picked
}

struct Field<'a> {
    name: &'a str,
    value: &'a str,
    /// What the field says when it is empty, which is what leaving it empty means.
    hint: &'a str,
    explanation: &'a str,
}

/// Draw a field, and answer what was typed when it changed.
fn field(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pen: &mut f32,
    settings_field: Field<'_>,
) -> Option<String> {
    let painter = ui.painter().clone();
    let at = Rect::from_min_size(Pos2::new(area.min.x + PAD, *pen), Vec2::new(area.width() - PAD * 2.0, ROW));
    named(ui, &painter, at.min, settings_field.name, look);
    let box_at = Rect::from_min_size(
        Pos2::new(at.min.x, at.min.y + look.font_size + 6.0),
        Vec2::new(VALUE, 24.0),
    );
    painter.rect(
        box_at,
        CornerRadius::same(look.corner_radius as u8),
        look.palette.field,
        egui::Stroke::new(1.0, look.palette.control_border),
        egui::StrokeKind::Inside,
    );
    let mut value = settings_field.value.to_owned();
    let response = ui.put(
        crate::components::controls::field_text_rect(ui, box_at, 8.0),
        egui::TextEdit::singleline(&mut value)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new(settings_field.hint).color(look.palette.text_faint))
            .font(egui::FontId::proportional(look.font_size - 0.5))
            .text_color(look.palette.text),
    );
    let copy = Rect::from_min_size(Pos2::new(box_at.max.x + 8.0, box_at.min.y), Vec2::new(54.0, 22.0));
    let copied = ui
        .push_id((settings_field.name, "copy"), |ui| {
            crate::components::controls::choice_button(ui, copy, "Copy", false)
        })
        .inner;
    *pen = explain(&painter, look, at, box_at.max.y + 6.0, settings_field.explanation);
    if copied {
        // Copying is not a change, so it is not reported as one: the caller writes what changed.
        ui.ctx().copy_text(settings_field.value.to_owned());
        return None;
    }
    response.changed().then_some(value)
}

/// A setting's name, drawn and named.
fn named(ui: &mut egui::Ui, painter: &egui::Painter, at: Pos2, said: &str, look: &Look<'_>) {
    let width = text(painter, at, said, look.font_size, look.palette.text_strong);
    let response = ui.interact(
        Rect::from_min_size(at, Vec2::new(width, look.font_size + 2.0)),
        ui.id().with(("agent-tasks-setting", said)),
        egui::Sense::hover(),
    );
    let name = said.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name.clone()));
}
