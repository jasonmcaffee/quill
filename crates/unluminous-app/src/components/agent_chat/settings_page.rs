//! The Agent-Chat page in the Settings window.
//!
//! The ticket's fourth ask: "a config in settings to allow configure url for Claude, codex etc." So
//! an endpoint is a **row in a list** rather than a constant in the binary, and the three that ship
//! are only the three rows that are there the first time this page is opened. Every field of every
//! one is editable, rows can be added and taken away, and one of them is the one that is used.
//!
//! Built from `components::modal`'s own furniture — its sections, notes, fields, tick boxes and
//! buttons — so this is the same page as the other six rather than a seventh that almost agrees with
//! them. It scrolls, which `components/agent_tasks/settings_page.rs` established for a page with more
//! in it than the 640 points every page gets.
//!
//! ## A row is a program or an address, and it draws the fields that apply
//!
//! The two rows that ship run the **command-line agent** installed on this machine, so what they
//! need is a program and how much it may do — and a URL, a key and a token budget are four fields
//! that mean nothing there. They are **absent** rather than dimmed for such a row, which is Unluminous's
//! own rule for a control that can never apply.
//!
//! ## No key is drawn and no key is written
//!
//! A row that sends to an address says `set` or `not set` and never the value, because a page that
//! showed it would be a page somebody screenshots. What is typed here is the **name of an
//! environment variable**; the value is read at the moment a request is sent and is never held,
//! written down or logged. That is `services::agent_tasks::keychain`'s rule, and it is what makes it
//! safe for this file to be copied between machines. A row that runs a program has no key at all —
//! the agent holds its own.

use egui::{CornerRadius, Pos2, Rect, Stroke, Vec2};

use unluminous_chat::provider::{Provider, Wire, DEFAULT_MAX_TOKENS, WIRES};

use crate::services::agent_chat::AgentChat;
use crate::services::plugin_ui::{Look, Request};
use crate::theme::color;

const PAD: f32 = 12.0;
/// How wide one of the wire-shape buttons is, and the gap between two of them.
///
/// Narrower than the 74 the first version used, because there are five of them now: at 74 the row
/// was 390 points wide against the 350 there is between the name field and `Use`, and `responses`
/// was drawn underneath `Use`. Measured against the rendered page rather than guessed at.
const WIRE_BUTTON: f32 = 60.0;
const WIRE_GAP: f32 = 4.0;
/// A field's own height, and the gap under a row.
const FIELD: f32 = 26.0;
const GAP: f32 = 10.0;
/// How wide the label column is, so every field on the page starts at the same x.
const LABEL: f32 = 96.0;

/// Draw the page inside the rectangle every page gets.
pub fn show(chat: &mut AgentChat, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let height = ui.available_rect_before_wrap().height();
    let scrolled = egui::ScrollArea::vertical()
        .id_salt("agent-chat-settings")
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| rows(chat, ui, look));
    scrolled.inner
}

/// The rows themselves.
///
/// The rectangle comes from the `Ui` this is given rather than from the caller, because inside a
/// scrolling area that `Ui`'s origin is where the scroll offset has put it — the fault
/// `components/agent_tasks/settings_page.rs` records, where the bar moved and the page did not.
fn rows(chat: &mut AgentChat, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let inner = Rect::from_min_max(
        Pos2::new(area.left() + PAD, area.top() + PAD),
        Pos2::new(area.right() - PAD, area.bottom()),
    );
    let mut requests = Vec::new();
    let mut pen = inner.top();
    // **Asked once a frame at most, and cached for five seconds under that.** For a program row the
    // answer is a walk of `PATH`; asked once a row once a frame it was a directory listing inside the
    // draw. See `AgentChat::readiness`.
    let refusals = chat.readiness();
    let mut configuration = chat.configuration().clone();
    let mut changed = false;
    let chosen = configuration
        .provider()
        .map(|one| one.name.clone())
        .unwrap_or_default();

    pen = crate::components::modal::section(ui, inner, pen, "Endpoints");
    pen = crate::components::modal::note(
        ui,
        inner,
        pen,
        "What answers. `claude-cli` and `codex-cli` run the agent already installed on this machine: \
         they need no key, they bring their own tools, and they read the open project's own \
         instructions. `openai`, `anthropic` and `responses` send to a URL instead — for those a key \
         is never stored by Unluminous, so name the environment variable it is in.",
    );

    let mut removing: Option<usize> = None;
    for index in 0..configuration.providers.len() {
        // **A child `Ui` an endpoint, carrying its own id salt.** Every button and field here is drawn
        // by `components::modal`, which derives a widget's id from `ui.id()` and its name — so three
        // rows each holding a `Use`, a `Remove` and an `openai` would be three widgets sharing one id,
        // which egui reports as a duplicate and which makes the second row's buttons unclickable.
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .id_salt(("agent-chat-endpoint", index)),
        );
        let why_not = refusals.get(index).cloned().flatten();
        let (height, act) =
            endpoint(&mut row, look, inner, pen, &mut configuration, index, &chosen, why_not);
        pen += height;
        match act {
            Some(Act::Remove) => removing = Some(index),
            Some(Act::Use) => {
                let name = configuration.providers[index].name.clone();
                configuration.chosen = name;
                changed = true;
            }
            Some(Act::Changed) => changed = true,
            None => {}
        }
    }
    if let Some(index) = removing {
        // The last one is never taken away: a pane with no endpoint cannot do anything, and there
        // would then be no row to type a new one into.
        if configuration.providers.len() > 1 {
            configuration.providers.remove(index);
            changed = true;
        } else {
            requests.push(Request::Message(
                "There has to be one endpoint. Change this one rather than taking it away.".to_owned(),
            ));
        }
    }

    let add = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(120.0, FIELD));
    if crate::components::modal::button(ui, add, "Add endpoint", true, false) {
        let mut new = Provider::defaults()[2].clone();
        new.name = unused_name(&configuration.providers);
        configuration.providers.push(new);
        changed = true;
    }
    pen += FIELD + GAP * 2.0;

    // **Which section is drawn depends on what the chosen row is**, because the two transports ask
    // opposite questions. An agent is a whole program with its own tools and its own sandbox, so the
    // only question is how much it may do; an endpoint is a model on the end of a socket, so the
    // question is which of Unluminous's own commands it may ask for. Drawing both would be a page with
    // four controls on it of which two can never apply — the absent-control rule, made once here
    // rather than four times.
    let an_agent = configuration.provider().is_some_and(|one| one.is_a_program());

    pen = crate::components::modal::section(ui, inner, pen, "Answering");

    if an_agent {
        pen = crate::components::modal::note(
            ui,
            inner,
            pen,
            "The chosen row is a program on this machine, so it answers with its own tools, its own sandbox \
             and its own account. It runs in the project this window has open and reads that \
             project's own instructions.",
        );
        pen = permission_row(ui, inner, pen, &mut configuration, &mut changed);
    } else {
        let row = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(inner.width(), 22.0));
        if crate::components::modal::check(ui, row, "Stream the answer", &mut configuration.stream) {
            changed = true;
        }
        pen += 22.0;
        pen = crate::components::modal::note(
            ui,
            inner,
            pen,
            "On, the answer arrives a word at a time. Off, it arrives whole — which is what a proxy \
             that will not stream needs. A program always streams.",
        );

        let row = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(inner.width(), 22.0));
        if crate::components::modal::check(
            ui,
            row,
            "Let the model use Unluminous's own commands",
            &mut configuration.tools,
        ) {
            changed = true;
        }
        pen += 22.0;
        pen = crate::components::modal::note(
            ui,
            inner,
            pen,
            "Off unless you say so. On, the model is offered Unluminous's whole command catalogue as \
             tools, so it can open a file, read the git status or run a search — through exactly the \
             code a menu entry runs. The same switch is the first button in the composer.",
        );

        let row = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(inner.width(), 22.0));
        if crate::components::modal::check(
            ui,
            row,
            "…including the ones that run a program",
            &mut configuration.shell,
        ) {
            changed = true;
        }
        pen += 22.0;
        pen = crate::components::modal::note(
            ui,
            inner,
            pen,
            "A second switch, because typing into a terminal, starting a run configuration and \
             installing a debug adapter hand this machine's shell to whatever is on the far end of \
             that URL.",
        );

        let mut limit = configuration.tool_limit.to_string();
        if let Some(typed) = one_field(ui, look, inner, &mut pen, "Tool rounds", &mut limit, "8") {
            if let Ok(rounds) = typed.trim().parse::<u32>() {
                if rounds > 0 {
                    configuration.tool_limit = rounds.min(32);
                    changed = true;
                }
            }
        }
        pen = crate::components::modal::note(
            ui,
            inner,
            pen,
            "How many times in one turn the model may ask for tools before the pane stops asking \
             again.",
        );
    }

    let mut system = configuration.system.clone();
    if one_field(ui, look, inner, &mut pen, "System", &mut system, "nothing").is_some() {
        configuration.system = system;
        changed = true;
    }
    pen = crate::components::modal::note(
        ui,
        inner,
        pen,
        "Added after Unluminous's own line, which says which file is showing. The file's text is never \
         sent by Unluminous.",
    );

    let mut history = configuration.history.to_string();
    if let Some(typed) = one_field(ui, look, inner, &mut pen, "History", &mut history, "20") {
        if let Ok(count) = typed.trim().parse::<usize>() {
            if count > 0 {
                configuration.history = count.min(500);
                changed = true;
            }
        }
    }
    pen = crate::components::modal::note(ui, inner, pen, "How many conversations are kept.");
    let _ = pen;

    if changed {
        *chat.configuration_mut() = configuration;
        // A row that was just edited may name a different program, so the next frame asks again
        // rather than repeating what it said about the one it used to name.
        chat.readiness_may_have_changed();
        if let Err(problem) = chat.save_the_configuration() {
            requests.push(Request::Message(problem));
        }
    }
    requests
}

/// What a command-line agent may do, as three buttons.
///
/// Buttons rather than a dropdown for the reason the wire shapes are buttons: there are three, and
/// which one is chosen is the most consequential thing on this page, so it should be readable
/// without opening anything.
fn permission_row(
    ui: &mut egui::Ui,
    area: Rect,
    top: f32,
    configuration: &mut crate::services::agent_chat::Configuration,
    changed: &mut bool,
) -> f32 {
    crate::components::modal::label(
        &ui.painter_at(area),
        Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), FIELD)),
        area.left(),
        "May",
        color::text_dim(),
        11.5,
    );
    let mut left = area.left() + LABEL;
    for named in unluminous_chat::PERMISSIONS {
        let at = Rect::from_min_size(Pos2::new(left, top), Vec2::new(84.0, FIELD));
        let on = configuration.permission.name() == *named;
        if crate::components::controls::choice_button(ui, at, named, on) {
            if let Some(permission) = unluminous_chat::Permission::from_name(named) {
                configuration.permission = permission;
                *changed = true;
            }
        }
        left += 90.0;
    }
    // **What each agent's answer to this really is**, because the two are not the same strength and
    // one label over both would promise something `claude` does not enforce: `codex` is put in an
    // operating system sandbox, and `claude` is told a policy about its own tools while its process,
    // its hooks and its plugins are not limited at all.
    let said = match configuration.permission {
        unluminous_chat::Permission::Read => {
            "Look and answer, changing nothing. `codex` runs in a read-only sandbox; `claude` refuses              any tool of its own that would change something."
        }
        unluminous_chat::Permission::Edit => {
            "Change files in the project this window has open. `codex` is sandboxed to it; `claude`              accepts its own edits without asking."
        }
        unluminous_chat::Permission::Full => {
            "Anything at all: edit any file and run any command, with no sandbox and nothing to ask."
        }
    };
    crate::components::modal::note(ui, area, top + FIELD + 4.0, said)
}

/// What one endpoint's row reported.
enum Act {
    Changed,
    Use,
    Remove,
}

/// One endpoint: its name, where it is, what it speaks and where its key comes from.
///
/// Answers how tall it drew, so the page's pen moves by what was really drawn rather than by a
/// constant that has to be kept in step with it.
fn endpoint(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    top: f32,
    configuration: &mut crate::services::agent_chat::Configuration,
    index: usize,
    chosen: &str,
    why_not: Option<String>,
) -> (f32, Option<Act>) {
    let mut act = None;
    let mut pen = top + 4.0;
    let in_use = configuration.providers[index].name == chosen;
    // A card round each row, so a page of five endpoints reads as five things rather than as twenty
    // fields. The ground the code blocks in a preview already sit on.
    let card_top = pen;

    let head = Rect::from_min_size(
        Pos2::new(area.left() + 8.0, pen + 6.0),
        Vec2::new(area.width() - 16.0, FIELD),
    );
    let mut name = configuration.providers[index].name.clone();
    let name_at = Rect::from_min_size(head.min, Vec2::new(140.0, FIELD));
    if crate::components::modal::field(ui, name_at, "Endpoint name", &mut name).changed() {
        configuration.providers[index].name = name.trim().to_owned();
        act = Some(Act::Changed);
    }
    // Which shape it speaks, as buttons rather than a dropdown: there are five and they are the
    // whole of what this row is, so a list that has to be opened would hide the answer.
    let mut left = name_at.right() + 8.0;
    for named in WIRES {
        let at = Rect::from_min_size(Pos2::new(left, head.top()), Vec2::new(WIRE_BUTTON, FIELD));
        let on = configuration.providers[index].wire.name() == *named;
        if crate::components::controls::choice_button(ui, at, named, on) {
            if let Some(wire) = Wire::from_name(named) {
                configuration.providers[index].wire = wire;
                // **Changing the shape fills in what that shape needs.** A row switched to
                // `codex-cli` with `https://api.anthropic.com/…` still in it is a row that says it
                // runs codex and names an address, which is a page telling two stories at once.
                let row = &mut configuration.providers[index];
                if wire.is_a_program() && row.command.trim().is_empty() {
                    row.command = match wire {
                        Wire::CodexCli => "codex".to_owned(),
                        _ => "claude".to_owned(),
                    };
                }
                act = Some(Act::Changed);
            }
        }
        left += WIRE_BUTTON + WIRE_GAP;
    }
    let use_at = Rect::from_min_size(
        Pos2::new(area.right() - 118.0, head.top()),
        Vec2::new(56.0, FIELD),
    );
    if crate::components::modal::button(ui, use_at, "Use", !in_use, in_use) {
        act = Some(Act::Use);
    }
    let remove_at = Rect::from_min_size(Pos2::new(area.right() - 58.0, head.top()), Vec2::new(50.0, FIELD));
    if crate::components::modal::button(ui, remove_at, "Remove", true, false) {
        act = Some(Act::Remove);
    }
    pen = head.bottom() + GAP;

    let inside = area.shrink2(Vec2::new(8.0, 0.0));
    let a_program = configuration.providers[index].is_a_program();
    if a_program {
        // **A program, and nothing else this row needs.** The agent holds its own key and picks its
        // own model unless one is named, so a URL, a key and a token budget are three fields that
        // could never do anything here.
        let mut command = configuration.providers[index].command.clone();
        if one_field(ui, look, inside, &mut pen, "Program", &mut command, "claude").is_some() {
            configuration.providers[index].command = command.trim().to_owned();
            act = Some(Act::Changed);
        }
    } else {
        let mut url = configuration.providers[index].url.clone();
        if one_field(ui, look, inside, &mut pen, "URL", &mut url, "https://…").is_some() {
            configuration.providers[index].url = url.trim().to_owned();
            act = Some(Act::Changed);
        }
    }
    let mut model = configuration.providers[index].model.clone();
    if one_field(
        ui,
        look,
        inside,
        &mut pen,
        "Model",
        &mut model,
        match a_program {
            // Empty is the honest default for an agent: it answers with the model its own person
            // chose, and naming one here would quietly override a choice made elsewhere.
            true => "the agent's own",
            false => "the model's name",
        },
    )
    .is_some()
    {
        configuration.providers[index].model = model.trim().to_owned();
        act = Some(Act::Changed);
    }
    if !a_program {
        let mut key = configuration.providers[index].key_env.clone();
        if one_field(ui, look, inside, &mut pen, "Key from", &mut key, "ANTHROPIC_API_KEY").is_some() {
            configuration.providers[index].key_env = key.trim().to_owned();
            act = Some(Act::Changed);
        }
        let mut budget = configuration.providers[index].max_tokens.to_string();
        if let Some(typed) = one_field(
            ui,
            look,
            inside,
            &mut pen,
            "Most tokens",
            &mut budget,
            &DEFAULT_MAX_TOKENS.to_string(),
        ) {
            if let Ok(count) = typed.trim().parse::<u32>() {
                configuration.providers[index].max_tokens = count.clamp(64, 200_000);
                act = Some(Act::Changed);
            }
        }
    }

    // Whether it can be reached, and never the key itself. `Provider::why_not` is the same sentence
    // the composer shows when a send is refused, so the page and the pane cannot disagree.
    let provider = &configuration.providers[index];
    let (said, tint) = match why_not {
        Some(why) => (why, color::close()),
        None => (
            match (provider.is_a_program(), provider.wants_a_key()) {
                // **Not the path it was found at**, though `why_the_program_will_not_run` knows it
                // and `plugins run agent-chat providers` reports it. A person's own home folder is
                // in that path, and this page is drawn into a screenshot test that is committed —
                // so the one place it would certainly end up is a repository.
                (true, _) => "Ready · found on this machine, and it holds its own account".to_owned(),
                (false, true) => format!("Ready · key set from ${}", provider.key_env),
                (false, false) => "Ready · no key needed".to_owned(),
            },
            color::git_added(),
        ),
    };
    let painter = ui.painter_at(area);
    painter.text(
        Pos2::new(area.left() + 8.0, pen),
        egui::Align2::LEFT_TOP,
        said,
        egui::FontId::proportional(11.5),
        tint,
    );
    pen += 20.0;

    // The card is painted **behind** what was drawn into it, which egui cannot do — so it is drawn
    // as a rectangle at the bottom of this widget's own layer, which is what `Painter::with_z` does
    // not offer here. Instead the border alone is drawn, which reads as a card without covering the
    // fields.
    painter.rect_stroke(
        Rect::from_min_max(
            Pos2::new(area.left(), card_top),
            Pos2::new(area.right(), pen - 4.0),
        ),
        CornerRadius::same(8),
        Stroke::new(
            1.0,
            match in_use {
                true => color::accent(),
                false => color::divider(),
            },
        ),
        egui::StrokeKind::Inside,
    );
    (pen + GAP - top, act)
}

/// A labelled field on one line, which is what every row on this page is.
///
/// Answers what was typed when it changed, and moves the pen past it.
fn one_field(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    area: Rect,
    pen: &mut f32,
    name: &str,
    value: &mut String,
    hint: &str,
) -> Option<String> {
    let row = Rect::from_min_size(Pos2::new(area.left(), *pen), Vec2::new(area.width(), FIELD));
    crate::components::modal::label(&ui.painter_at(area), row, row.left(), name, color::text_dim(), 11.5);
    let box_at = Rect::from_min_max(Pos2::new(row.left() + LABEL, row.top()), row.max);
    let response = crate::components::modal::field(ui, box_at, name, value);
    if value.trim().is_empty() {
        ui.painter_at(box_at).text(
            Pos2::new(box_at.left() + 8.0, box_at.center().y - look.font_size * 0.4),
            egui::Align2::LEFT_TOP,
            hint,
            egui::FontId::proportional(look.font_size * 0.82),
            color::text_faint(),
        );
    }
    *pen += FIELD + GAP * 0.6;
    match response.changed() {
        true => Some(value.clone()),
        false => None,
    }
}

/// A name no endpoint has yet, so `Add endpoint` twice makes two rows rather than two of one name.
fn unused_name(providers: &[Provider]) -> String {
    let mut count = providers.len() + 1;
    loop {
        let name = format!("endpoint-{count}");
        if !providers.iter().any(|one| one.name == name) {
            return name;
        }
        count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_an_endpoint_twice_makes_two_rows_rather_than_two_of_one_name() {
        let mut providers = Provider::defaults();
        let first = unused_name(&providers);
        let mut new = providers[2].clone();
        new.name = first.clone();
        providers.push(new);
        let second = unused_name(&providers);
        assert_ne!(first, second);
    }
}
