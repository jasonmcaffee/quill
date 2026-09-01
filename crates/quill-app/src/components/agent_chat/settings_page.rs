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
//! ## No key is drawn and no key is written
//!
//! A row says `set` or `not set` and never the value, because a page that showed it would be a page
//! somebody screenshots. What is typed here is the **name of an environment variable**; the value is
//! read at the moment a request is sent and is never held, written down or logged. That is
//! `services::agent_tasks::keychain`'s rule, and it is what makes it safe for this file to be copied
//! between machines.

use egui::{CornerRadius, Pos2, Rect, Stroke, Vec2};

use quill_chat::provider::{Provider, Wire, DEFAULT_MAX_TOKENS, WIRES};

use crate::services::agent_chat::AgentChat;
use crate::services::plugin_ui::{Look, Request};
use crate::theme::color;

const PAD: f32 = 12.0;
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
        "Where a message is sent. `openai` is the shape llama.cpp, LM Studio, Ollama and OpenAI all \
         speak; `anthropic` is Claude's own. A key is never stored by Quill — name the environment \
         variable it is in and it is read when a message is sent.",
    );

    let mut removing: Option<usize> = None;
    for index in 0..configuration.providers.len() {
        // **A child `Ui` an endpoint, carrying its own id salt.** Every button and field here is drawn
        // by `components::modal`, which derives a widget's id from `ui.id()` and its name — so three
        // rows each holding a `Use`, a `Remove` and an `openai` would be three widgets sharing one id,
        // which egui reports as a duplicate and which makes the second row's buttons unclickable.
        let mut row = ui.new_child(
            egui::UiBuilder::new().max_rect(area).id_salt(("agent-chat-endpoint", index)),
        );
        let (height, act) = endpoint(&mut row, look, inner, pen, &mut configuration, index, &chosen);
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

    pen = crate::components::modal::section(ui, inner, pen, "Answering");

    let row = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(inner.width(), 22.0));
    if crate::components::modal::check(ui, row, "Stream the answer", &mut configuration.stream) {
        changed = true;
    }
    pen += 22.0;
    pen = crate::components::modal::note(
        ui,
        inner,
        pen,
        "On, the answer arrives a word at a time. Off, it arrives whole — which is what a proxy that \
         will not stream needs.",
    );

    let row = Rect::from_min_size(Pos2::new(inner.left(), pen), Vec2::new(inner.width(), 22.0));
    if crate::components::modal::check(
        ui,
        row,
        "Let the model use Quill's own commands",
        &mut configuration.tools,
    ) {
        changed = true;
    }
    pen += 22.0;
    pen = crate::components::modal::note(
        ui,
        inner,
        pen,
        "Off unless you say so. On, the model is offered Quill's whole command catalogue as tools, so \
         it can open a file, read the git status or run a search — through exactly the code a menu \
         entry runs. The same switch is the first button in the composer.",
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
        "How many times in one turn the model may ask for tools before the pane stops asking again.",
    );

    let mut system = configuration.system.clone();
    if one_field(ui, look, inner, &mut pen, "System", &mut system, "nothing").is_some() {
        configuration.system = system;
        changed = true;
    }
    pen = crate::components::modal::note(
        ui,
        inner,
        pen,
        "Added after Quill's own line, which says which project is open and which file is showing. \
         The file's text is never sent.",
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
        if let Err(problem) = chat.save_the_configuration() {
            requests.push(Request::Message(problem));
        }
    }
    requests
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
    // Which shape it speaks, as buttons rather than a dropdown: there are two, and two buttons are
    // quicker to read than a list that has to be opened.
    let mut left = name_at.right() + 8.0;
    for named in WIRES {
        let at = Rect::from_min_size(Pos2::new(left, head.top()), Vec2::new(74.0, FIELD));
        let on = configuration.providers[index].wire.name() == *named;
        if crate::components::controls::choice_button(ui, at, named, on) {
            if let Some(wire) = Wire::from_name(named) {
                configuration.providers[index].wire = wire;
                act = Some(Act::Changed);
            }
        }
        left += 80.0;
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

    let mut url = configuration.providers[index].url.clone();
    if one_field(
        ui,
        look,
        area.shrink2(Vec2::new(8.0, 0.0)),
        &mut pen,
        "URL",
        &mut url,
        "https://…",
    )
    .is_some()
    {
        configuration.providers[index].url = url.trim().to_owned();
        act = Some(Act::Changed);
    }
    let mut model = configuration.providers[index].model.clone();
    if one_field(
        ui,
        look,
        area.shrink2(Vec2::new(8.0, 0.0)),
        &mut pen,
        "Model",
        &mut model,
        "the model's name",
    )
    .is_some()
    {
        configuration.providers[index].model = model.trim().to_owned();
        act = Some(Act::Changed);
    }
    let mut key = configuration.providers[index].key_env.clone();
    if one_field(
        ui,
        look,
        area.shrink2(Vec2::new(8.0, 0.0)),
        &mut pen,
        "Key from",
        &mut key,
        "ANTHROPIC_API_KEY",
    )
    .is_some()
    {
        configuration.providers[index].key_env = key.trim().to_owned();
        act = Some(Act::Changed);
    }
    let mut budget = configuration.providers[index].max_tokens.to_string();
    if let Some(typed) = one_field(
        ui,
        look,
        area.shrink2(Vec2::new(8.0, 0.0)),
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

    // Whether it can be reached, and never the key itself. `Provider::why_not` is the same sentence
    // the composer shows when a send is refused, so the page and the pane cannot disagree.
    let provider = &configuration.providers[index];
    let (said, tint) = match provider.why_not() {
        Some(why) => (why, color::CLOSE),
        None => (
            match provider.wants_a_key() {
                true => format!("Ready · key set from ${}", provider.key_env),
                false => "Ready · no key needed".to_owned(),
            },
            color::GIT_ADDED,
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
                true => color::ACCENT,
                false => color::DIVIDER,
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
    crate::components::modal::label(&ui.painter_at(area), row, row.left(), name, color::TEXT_DIM, 11.5);
    let box_at = Rect::from_min_max(Pos2::new(row.left() + LABEL, row.top()), row.max);
    let response = crate::components::modal::field(ui, box_at, name, value);
    if value.trim().is_empty() {
        ui.painter_at(box_at).text(
            Pos2::new(box_at.left() + 8.0, box_at.center().y - look.font_size * 0.4),
            egui::Align2::LEFT_TOP,
            hint,
            egui::FontId::proportional(look.font_size * 0.82),
            color::TEXT_FAINT,
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
