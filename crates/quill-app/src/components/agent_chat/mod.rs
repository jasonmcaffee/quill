//! The Agent-Chat pane: the one raised card, its header, the conversation and the composer.
//!
//! `_agent_output/task-1767-agent-chat/reference-chat.png` is the picture this is measured against
//! and `tasks/task-1767-agent-chat-tdd.md` §1 says how it was made. The structure is the ai-service
//! LLM chat page's own — `ChatPanel` wrapping `ChatHeader`, `ChatConversation` and `ChatComposer` —
//! wearing the dark neumorphic palette the Agent-Tasks board is drawn in.
//!
//! ## The drawing changes nothing, and `pane` is where the change happens
//!
//! Every function here that draws takes a rectangle, draws, and reports an [`Act`]; not one of them
//! changes the conversation. [`pane`] is the provider's own entry rather than a component — it is what
//! `UiProvider::pane` calls — and it is the one place the acts are applied, after everything has been
//! drawn. That split is what lets the whole surface be drawn from a borrow of the conversation while
//! the things that change it need a mutable one.
//!
//! ## The ground is the window's
//!
//! `show_the_plugin_panes` fills the pane and reserves the decoration's slot before this is called. A
//! second ground painted here would go into the painter *after* that slot and wash the decoration
//! out, which is the fault `task-1765` records for the board. So the only surface painted here is the
//! panel itself, through `Chrome::raised`.

pub mod composer;
pub mod message;
pub mod settings_page;

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};

use crate::services::agent_chat::{AgentChat, Parts};
use crate::services::plugin_ui::{Look, Request};
use crate::services::vello_canvas::{Fill, Lift};
use crate::theme::icon;

/// A colour moved towards black, which is the far end of a button's own gradient.
///
/// `components::agent_tasks` already has one and it is the same arithmetic; it is re-exported here so
/// that both plugins darken a colour the same way rather than by two functions that agree today.
pub(crate) use crate::components::agent_tasks::darken;

/// The gap round the panel, which is the gap the explorer already leaves.
pub const PAD: f32 = 8.0;
/// The panel's own padding, from `ChatPanel.module.css`.
pub const INNER: f32 = 10.0;
/// The panel's corner radius: `--r-lg`.
pub const RADIUS: f32 = 18.0;
/// The header row, from `ChatHeader.module.css`'s padding plus its 13 point name.
pub const HEADER: f32 = 32.0;
/// Between two rows of the conversation, from `ChatConversation.module.css`'s own `gap: 14px`.
pub const GAP: f32 = 14.0;

/// What the drawing reported, applied by [`pane`] once everything has been drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Send,
    Stop,
    New,
    Open(String),
    Remove(String),
    Choose(String),
    ToggleTools,
    ToggleStream,
    /// Ask for a picture, which opens the platform's own file picker.
    Attach,
    /// A picture dropped on the pane.
    Dropped(std::path::PathBuf),
    /// Ctrl/Cmd+V in the composer: ask the window for whatever picture is on the clipboard.
    Paste,
    /// A starter chip: it fills the prompt rather than sending it, which is what the page this is
    /// modelled on does.
    Starter(&'static str),
    Detach(u64),
    Copy(String),
    ShowHistory(bool),
    ShowProviders(bool),
    /// Open or close one tool block, by its call id.
    ToggleTool(String),
    /// Open or close one message's thinking.
    ToggleThinking(u64),
}

/// Draw the pane, and act on what was pressed.
pub fn pane(chat: &mut AgentChat, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
    let area = ui.available_rect_before_wrap();
    let mut acts = Vec::new();
    if area.width() > 40.0 && area.height() > 40.0 {
        acts = surface(chat.parts(), ui, look, area);
    }
    // The pointer's own answer to a picture dropped on the pane. `egui` collects what the window
    // manager handed over, and a path that is a picture is attached exactly as the `+` would attach
    // it — which is the third way in the ticket's own list, after the button and the clipboard.
    for path in dropped_pictures(ui, area) {
        acts.push(Act::Dropped(path));
    }
    // **A paste is claimed before the pane loop reads it**, which is the one-frame ordering the
    // Markdown preview's own copy already uses. What `egui` reports of Ctrl/Cmd+V when the clipboard
    // holds a picture rather than text is nothing at all, so the chord is read off the key going back
    // up and the window is asked for the picture - see `pasting`.
    if pasting(ui, chat.parts().state.prompt_focused) {
        acts.push(Act::Paste);
    }
    apply(chat, acts)
}

/// Everything inside the pane, from a borrow of the conversation.
fn surface(mut parts: Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let panel = area.shrink(PAD * scale);
    let radius = RADIUS * scale;
    // **One raised card holds the whole chat**, which is `ChatPanel.module.css`'s `.panel`. With the
    // decoration off it is the flat bordered panel every list in Quill draws, so switching the
    // renderer off in the manifest or in `plugins.chrome` really withdraws the depth.
    if look.chrome.is_recording() {
        look.chrome
            .raised(panel, radius, Fill::Solid(look.palette.board_lane), Lift::Small);
    } else {
        ui.painter().rect(
            panel,
            CornerRadius::same(radius as u8),
            look.ground(look.palette.board_lane),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let inner = panel.shrink(INNER * scale);
    if inner.width() < 40.0 {
        return acts;
    }

    let header_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER * scale));
    acts.extend(header(&parts, ui, look, header_rect));

    let composer_height = composer::height(&parts, look, inner.width());
    let composer_rect = Rect::from_min_size(
        Pos2::new(inner.left(), inner.bottom() - composer_height),
        Vec2::new(inner.width(), composer_height),
    );
    let body = Rect::from_min_max(
        Pos2::new(inner.left(), header_rect.bottom() + 4.0 * scale),
        Pos2::new(inner.right(), composer_rect.top() - 4.0 * scale),
    );
    if body.height() > 20.0 {
        // The two lists are drawn **over** the conversation rather than in a popup, because egui keeps
        // at most one popup open at a time — the rule that already shaped the flyouts, the colour wheel
        // and the completion list — and a pane that could not open its own history while a menu was up
        // would be a pane whose history is unreachable at the moment somebody wants it.
        if parts.state.history_open {
            acts.extend(history_list(&mut parts, ui, look, body));
        } else if parts.state.providers_open {
            acts.extend(provider_list(&parts, ui, look, body));
        } else {
            acts.extend(conversation(&mut parts, ui, look, body));
        }
    }
    acts.extend(composer::show(parts, ui, look, composer_rect));
    acts
}

/// The header: the state dot, the conversation's name, the endpoint chip, history and new.
fn header(parts: &Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let painter = ui.painter_at(area);
    let middle = area.center().y;

    // The dot is the state, which is the one thing the header says that changes by itself: mint when
    // it is ready, the board's blue while an answer is arriving, red when the last one failed. Its
    // halo is the reference's `box-shadow: 0 0 0 3px`, drawn as a glow rather than a ring because a
    // hard ring at this size reads as a second dot.
    let (tone, said) = state_of(parts, look);
    let dot = Pos2::new(area.left() + 5.0 * scale, middle);
    if look.chrome.is_recording() {
        look.chrome.glow(
            Rect::from_center_size(dot, Vec2::splat(4.5 * scale)),
            4.5 * scale,
            tone.gamma_multiply(0.35),
            5.0 * scale,
        );
        look.chrome.disc(dot, 4.0 * scale, Fill::Solid(tone));
    } else {
        painter.circle_filled(dot, 4.0 * scale, tone);
    }

    let name = parts.session.chat.display_name();
    let mut pen = dot.x + 10.0 * scale;
    // How much room the name may take: whatever the two buttons and the chip leave it.
    let chip_width = (parts
        .configuration
        .provider()
        .map(|one| one.name.len())
        .unwrap_or(0) as f32
        * 5.6
        + 16.0)
        * scale;
    let buttons = 56.0 * scale;
    let room = (area.right() - pen - chip_width - buttons - 12.0 * scale).max(24.0);
    // **Laid out without wrapping and then clipped**, because a conversation named after a long first
    // sentence has to lose its end rather than gain a second line: a wrapped name drew over the chip
    // beside it and over the first message under it.
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(look.font_size * 0.82),
        look.palette.text_strong,
    );
    let cut = galley.size().x.min(room);
    painter
        .with_clip_rect(Rect::from_min_size(
            Pos2::new(pen, area.top()),
            Vec2::new(room, area.height()),
        ))
        .galley(
            Pos2::new(pen, middle - look.font_size * 0.55),
            galley,
            look.palette.text_strong,
        );
    pen += cut + 10.0 * scale;

    // The endpoint's own chip, which is `ChatHeader.module.css`'s datasource chip: a pressed well in
    // mono, uppercase. Pressing it opens the list of endpoints.
    if let Some(provider) = parts.configuration.provider() {
        let chip = Rect::from_min_size(
            Pos2::new(area.right() - buttons - chip_width, middle - 9.0 * scale),
            Vec2::new(chip_width, 18.0 * scale),
        );
        if chip.left() > pen {
            let response = ui.interact(chip, ui.id().with("agent-chat-provider"), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Endpoint: {}", provider.name),
                )
            });
            if look.chrome.is_recording() {
                look.chrome
                    .sunken(chip, 9.0 * scale, look.palette.board_well, Lift::Small);
            } else {
                painter.rect_filled(
                    chip,
                    CornerRadius::same((9.0 * scale) as u8),
                    look.palette.board_well,
                );
            }
            let tint = match response.hovered() {
                true => look.palette.text_strong,
                false => look.palette.attached,
            };
            let words = provider.name.to_uppercase();
            let width = painter
                .layout_no_wrap(
                    words.clone(),
                    egui::FontId::monospace(look.font_size * 0.62),
                    tint,
                )
                .size()
                .x;
            painter.text(
                Pos2::new(
                    chip.center().x - width / 2.0,
                    chip.center().y - look.font_size * 0.4,
                ),
                egui::Align2::LEFT_TOP,
                words,
                egui::FontId::monospace(look.font_size * 0.62),
                tint,
            );
            if response.clicked() {
                acts.push(Act::ShowProviders(!parts.state.providers_open));
            }
        }
    }

    // Two ghost buttons, which is `ChatHeader.module.css`'s `.toolBtn`: no surface until the pointer
    // is on them.
    let history = Rect::from_center_size(
        Pos2::new(area.right() - 38.0 * scale, middle),
        Vec2::splat(22.0 * scale),
    );
    if crate::components::controls::icon_button(ui, history, "Conversations", icon::clock) {
        acts.push(Act::ShowHistory(!parts.state.history_open));
    }
    let new = Rect::from_center_size(
        Pos2::new(area.right() - 12.0 * scale, middle),
        Vec2::splat(22.0 * scale),
    );
    if crate::components::controls::icon_button(ui, new, "New Conversation", icon::plus) {
        acts.push(Act::New);
    }

    // The hairline under it, which is the reference's `border-bottom`.
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(area.left(), area.bottom() - 1.0),
            Pos2::new(area.right(), area.bottom()),
        ),
        0,
        look.palette.divider,
    );
    let _ = said;
    acts
}

/// The colour of the state dot, and the word for it.
fn state_of(parts: &Parts<'_>, look: &Look<'_>) -> (Color32, &'static str) {
    use quill_chat::State;
    match parts.session.state() {
        State::Failed(_) => (crate::theme::color::close(), "failed"),
        State::Sending | State::Streaming => (look.palette.board_accent, "answering"),
        State::WaitingForTools => (crate::theme::color::agent(), "running a tool"),
        _ => (crate::theme::color::git_added(), "ready"),
    }
}

/// The conversation: every message, scrolled, with the empty state when there is nothing.
fn conversation(parts: &mut Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    // Copied out of the parts so the conversation can be read while the little the drawing remembers
    // is written into. Both are borrows of different fields, which is what makes this legal at all.
    let session: &quill_chat::Session = parts.session;
    // Asked for once and then cleared, which is what `reveal_caret` does: a jump that ran on every
    // frame would make the conversation impossible to scroll at all.
    let jump = std::mem::take(&mut parts.state.jump_to_bottom);
    if session.chat.messages.is_empty() {
        return empty(ui, look, area);
    }
    let mut acts = Vec::new();
    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(area));
    body.set_clip_rect(area);
    // **The decoration is cut to the conversation, and it has to be.** A `Chrome` records absolute
    // rectangles into one canvas that covers the whole pane, so a bubble scrolled half out of view
    // recorded its whole surface and the canvas painted it over the header above. `egui`'s own clip
    // rectangle cannot reach the canvas; `Decor::Clip` is the one thing that can. Measured on a real
    // window: a message scrolled off the top was drawn across the pane's own name.
    look.chrome.clip(area, 0.0);
    let mut scroller = egui::ScrollArea::vertical()
        .id_salt("agent-chat-conversation")
        // **Stuck to the bottom while an answer is arriving, and unstuck the moment somebody scrolls
        // up** — which is `ChatPage.tsx`'s own `shouldAutoScroll` rule. egui's own stickiness does
        // exactly that: it follows while the view is already at the bottom and stops when it is not.
        .stick_to_bottom(true)
        .auto_shrink([false, false]);
    if jump {
        // What stickiness will not do is go *back* to the bottom once somebody has scrolled away, and
        // sending, opening a conversation and starting a new one all have to.
        scroller = scroller.vertical_scroll_offset(f32::MAX);
    } else if let Some(offset) = parts.state.scroll_to.take() {
        // A zoom moved everything, so the point that was under the pointer is put back under it. The same
        // one-shot shape, worked out by `AgentChat::zoomed`. `task-1771`.
        scroller = scroller.vertical_scroll_offset(offset.max(0.0));
    }
    let scrolled = scroller.show(&mut body, |ui| {
        let width = area.width();
        for one in &session.chat.messages {
            // A tool result is the copy that goes back up the wire; it is drawn inside the block
            // of the call it answers, so it is not a row of its own.
            if one.role == quill_chat::Role::Tool {
                continue;
            }
            // Worked out once and handed to the drawing, because the height has to be known
            // before the rectangle can be allocated and running it twice built the message's text
            // twice. See `message::Shape`.
            let shape = message::shape(one, parts.state, look, width);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, shape.height), egui::Sense::hover());
            // **Only what can be seen is drawn**, which is `task-1666`'s rule and, here, also what
            // keeps the decoration's canvas the size of the pane: a bubble scrolled a thousand
            // points away would otherwise record shadows a thousand points outside it.
            if rect.intersects(ui.clip_rect()) {
                acts.extend(message::show(one, shape, parts.state, ui, look, rect));
            }
            ui.add_space(GAP * look.scale());
        }
    });
    // Where the conversation was left, so a zoom can put it back where it was. See `PaneState::scrolled`.
    parts.state.scrolled = scrolled.state.offset.y;
    look.chrome.unclip();
    acts
}

/// What the pane says when nothing has been said in it.
///
/// `ChatPage.module.css`'s `.chatEmpty`, with the four starter prompts made about what Quill is: a
/// chip fills the prompt rather than sending it, which is what the page it comes from does.
fn empty(ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let painter = ui.painter_at(area);
    let mut pen = area.top() + (area.height() * 0.16).min(60.0);
    let badge = Rect::from_center_size(
        Pos2::new(area.center().x, pen + 28.0 * scale),
        Vec2::splat(56.0 * scale),
    );
    if look.chrome.is_recording() {
        look.chrome
            .sunken(badge, 16.0 * scale, look.palette.board_card, Lift::Small);
    } else {
        painter.rect_filled(
            badge,
            CornerRadius::same((16.0 * scale) as u8),
            look.palette.board_card,
        );
    }
    icon::chat(&painter, badge.center(), look.palette.board_accent);
    pen = badge.bottom() + 14.0 * scale;
    centred(
        &painter,
        area,
        pen,
        "How can I help?",
        look.font_size * 1.2,
        look.palette.text_strong,
    );
    pen += look.font_size * 1.7;
    centred(
        &painter,
        area,
        pen,
        "Ask anything about what is open.",
        look.font_size * 0.82,
        look.palette.text_dim,
    );
    pen += look.font_size * 1.9;

    let mut acts = Vec::new();
    let width = (area.width() - 12.0 * scale).min(300.0 * scale);
    for prompt in STARTERS {
        let chip = Rect::from_min_size(
            Pos2::new(area.center().x - width / 2.0, pen),
            Vec2::new(width, 26.0 * scale),
        );
        if chip.bottom() > area.bottom() {
            break;
        }
        let response = ui.interact(
            chip,
            ui.id().with(("agent-chat-starter", prompt)),
            egui::Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, prompt.to_owned()));
        let ground = match response.hovered() {
            true => look.palette.selected_row,
            false => look.palette.board_card,
        };
        if look.chrome.is_recording() {
            look.chrome
                .raised(chip, 13.0 * scale, Fill::Solid(ground), Lift::Small);
        } else {
            painter.rect_filled(chip, CornerRadius::same((13.0 * scale) as u8), ground);
        }
        centred(
            &painter,
            chip,
            chip.center().y - look.font_size * 0.45,
            prompt,
            look.font_size * 0.85,
            look.palette.text_control,
        );
        if response.clicked() {
            acts.push(Act::Starter(prompt));
        }
        pen += 32.0 * scale;
    }
    acts
}

/// The four chips on an empty pane. What a person asks an editor, rather than what they ask a
/// general chat: `ChatPage.tsx` has its own four and these are the same idea about this program.
pub const STARTERS: [&str; 4] = [
    "Explain this file",
    "Find the bug",
    "Write a test",
    "Summarise the diff",
];

/// A line of text centred in `area` at `y`.
fn centred(painter: &egui::Painter, area: Rect, y: f32, said: &str, size: f32, tint: Color32) {
    let galley = painter.layout_no_wrap(said.to_owned(), egui::FontId::proportional(size), tint);
    let at = Pos2::new(area.center().x - galley.size().x / 2.0, y);
    painter.galley(at, galley, tint);
}

/// The conversations kept, drawn over the conversation area.
fn history_list(parts: &mut Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let session: &quill_chat::Session = parts.session;
    let history = parts.history;
    let mut acts = Vec::new();
    let scale = look.scale();
    let painter = ui.painter_at(area);
    if history.is_empty() {
        centred(
            &painter,
            area,
            area.top() + 20.0 * scale,
            "No conversations yet.",
            look.font_size * 0.85,
            look.palette.text_dim,
        );
    }
    let row = look.row_height * scale;
    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(area));
    body.set_clip_rect(area);
    egui::ScrollArea::vertical()
        .id_salt("agent-chat-history")
        .auto_shrink([false, false])
        .show(&mut body, |ui| {
            for one in history {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(area.width(), row), egui::Sense::hover());
                if !rect.intersects(ui.clip_rect()) {
                    continue;
                }
                let chosen = one.id == session.chat.id;
                let response = ui.interact(
                    rect,
                    ui.id().with(("agent-chat-history", &one.id)),
                    egui::Sense::click(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Conversation: {}", one.name),
                    )
                });
                let painter = ui.painter_at(rect);
                if chosen || response.hovered() {
                    painter.rect_filled(
                        rect.shrink2(Vec2::new(2.0, 1.0)),
                        CornerRadius::same(6),
                        match chosen {
                            true => look.palette.selected_row,
                            false => look.palette.control.gamma_multiply(0.5),
                        },
                    );
                }
                let cross = Rect::from_center_size(
                    Pos2::new(rect.right() - 12.0 * scale, rect.center().y),
                    Vec2::splat(18.0 * scale),
                );
                painter
                    .with_clip_rect(Rect::from_min_max(
                        rect.min,
                        Pos2::new(cross.left() - 4.0, rect.max.y),
                    ))
                    .text(
                        Pos2::new(rect.left() + 8.0 * scale, rect.center().y - look.font_size * 0.42),
                        egui::Align2::LEFT_TOP,
                        &one.name,
                        egui::FontId::proportional(look.font_size * 0.85),
                        look.palette.text_control,
                    );
                if response.clicked() {
                    acts.push(Act::Open(one.id.clone()));
                }
                if crate::components::controls::icon_button(
                    ui,
                    cross,
                    &format!("Remove conversation: {}", one.name),
                    icon::cross,
                ) {
                    acts.push(Act::Remove(one.id.clone()));
                }
            }
        });
    acts
}

/// The endpoints, drawn over the conversation area.
fn provider_list(parts: &Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let mut acts = Vec::new();
    let scale = look.scale();
    let row = look.row_height * scale + 12.0 * scale;
    let mut pen = area.top() + 4.0 * scale;
    for provider in &parts.configuration.providers {
        let rect = Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), row));
        if rect.bottom() > area.bottom() {
            break;
        }
        let chosen = parts
            .configuration
            .provider()
            .is_some_and(|one| one.name == provider.name);
        let response = ui.interact(
            rect,
            ui.id().with(("agent-chat-endpoint", &provider.name)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Talk to {}", provider.name),
            )
        });
        let painter = ui.painter_at(rect);
        let ground = match chosen {
            true => look.palette.selected_row,
            false => look.palette.board_card,
        };
        if chosen || response.hovered() {
            if look.chrome.is_recording() {
                look.chrome
                    .raised(rect.shrink(2.0), 10.0 * scale, Fill::Solid(ground), Lift::Small);
            } else {
                painter.rect_filled(rect.shrink(2.0), CornerRadius::same(10), ground);
            }
        }
        painter.text(
            Pos2::new(rect.left() + 10.0 * scale, rect.top() + 6.0 * scale),
            egui::Align2::LEFT_TOP,
            &provider.name,
            egui::FontId::proportional(look.font_size * 0.9),
            look.palette.text_strong,
        );
        // What is wrong with it, if anything, rather than a row that silently will not work when it
        // is pressed. `Provider::why_not` is the same sentence the composer shows.
        let (said, tint) = match provider.why_not() {
            Some(why) => (why, crate::theme::color::close()),
            None => (
                format!("{} · {}", provider.model, provider.wire.name()),
                look.palette.text_dim,
            ),
        };
        painter.with_clip_rect(rect).text(
            Pos2::new(
                rect.left() + 10.0 * scale,
                rect.top() + 6.0 * scale + look.font_size,
            ),
            egui::Align2::LEFT_TOP,
            said,
            egui::FontId::proportional(look.font_size * 0.72),
            tint,
        );
        if response.clicked() {
            acts.push(Act::Choose(provider.name.clone()));
        }
        pen += row + 4.0 * scale;
    }
    acts
}

/// Do what the drawing reported, and answer what the window has to do.
fn apply(chat: &mut AgentChat, acts: Vec<Act>) -> Vec<Request> {
    let mut requests = Vec::new();
    for act in acts {
        match act {
            Act::Send => {
                if let Err(problem) = chat.send() {
                    requests.push(Request::Message(problem));
                }
            }
            Act::Stop => chat.stop(),
            Act::New => {
                chat.new_conversation();
                chat.ui.history_open = false;
            }
            Act::Starter(prompt) => chat.draft = prompt.to_owned(),
            Act::Dropped(path) => {
                if let Err(problem) = chat.attach(&path) {
                    requests.push(Request::Message(problem));
                }
            }
            Act::Paste => requests.push(Request::ClipboardPicture {
                id: crate::services::agent_chat::CLIPBOARD.to_owned(),
            }),
            Act::Open(id) => {
                if let Err(problem) = chat.open_conversation(&id) {
                    requests.push(Request::Message(problem));
                }
                chat.ui.history_open = false;
            }
            Act::Remove(id) => {
                if let Err(problem) = chat.remove_conversation(&id) {
                    requests.push(Request::Message(problem));
                }
            }
            Act::Choose(name) => {
                if let Err(problem) = chat.configuration_mut().choose(&name) {
                    requests.push(Request::Message(problem));
                }
                if let Err(problem) = chat.save_the_configuration() {
                    requests.push(Request::Message(problem));
                }
                chat.ui.providers_open = false;
            }
            Act::ToggleStream => {
                let now = !chat.configuration().stream;
                chat.configuration_mut().stream = now;
                if let Err(problem) = chat.save_the_configuration() {
                    requests.push(Request::Message(problem));
                }
                requests.push(Request::Message(match now {
                    true => "The answer arrives a word at a time.".to_owned(),
                    false => "The answer arrives whole.".to_owned(),
                }));
            }
            Act::ToggleTools => {
                let now = !chat.configuration().tools;
                chat.configuration_mut().tools = now;
                if let Err(problem) = chat.save_the_configuration() {
                    requests.push(Request::Message(problem));
                }
                requests.push(Request::Message(match now {
                    true => "Quill's own commands are offered to the model.".to_owned(),
                    false => "No tools are offered.".to_owned(),
                }));
            }
            // The picker is the platform's, and `rfd` is how Quill already opens one. It blocks the
            // frame it is opened in, exactly as `File -> Open` does.
            Act::Attach => match pick_a_picture() {
                Some(path) => {
                    if let Err(problem) = chat.attach(&path) {
                        requests.push(Request::Message(problem));
                    }
                }
                None => {}
            },
            Act::Detach(id) => chat.remove_attachment(id),
            Act::Copy(text) if !text.is_empty() => requests.push(Request::Copy(text)),
            Act::Copy(_) => {}
            Act::ShowHistory(open) => {
                chat.ui.history_open = open;
                chat.ui.providers_open = false;
            }
            Act::ShowProviders(open) => {
                chat.ui.providers_open = open;
                chat.ui.history_open = false;
            }
            Act::ToggleTool(id) => match chat.ui.opened_tools.iter().position(|one| *one == id) {
                Some(at) => {
                    chat.ui.opened_tools.remove(at);
                }
                None => chat.ui.opened_tools.push(id),
            },
            Act::ToggleThinking(id) => match chat.ui.opened_thinking.iter().position(|one| *one == id) {
                Some(at) => {
                    chat.ui.opened_thinking.remove(at);
                }
                None => chat.ui.opened_thinking.push(id),
            },
        }
    }
    requests
}

/// Whether the composer was pasted into this frame.
///
/// Only while a field in this pane has the keyboard, so a paste meant for the editing area is not
/// taken.
///
/// **It is the key going back up that says so, and it took reading `egui-winit` to find out why.**
/// This used to watch for an *empty* `egui::Event::Paste`, on the reasoning that a picture is not text
/// and would therefore arrive as a paste with nothing in it. `egui-winit` does not do that. Its
/// `on_keyboard_input` recognises the paste chord, reads the clipboard's **text**, and pushes
/// `Event::Paste` only `if !contents.is_empty()` — then returns, so the `V` key press is swallowed as
/// well. With a picture on the clipboard `get_text()` fails outright and the frame carries **no event
/// at all**. So the condition that was being watched for could never once have happened, and pasting a
/// picture into this pane has never worked. `task-1771`.
///
/// The `return` is only taken while the key is **down**. The release comes back through the ordinary
/// path as `Event::Key { key: V, pressed: false, modifiers }`, with the modifier still held, which is
/// the one report of the chord that reaches Quill. Asking the window for a picture on the way up is a
/// keystroke late and nobody can tell; a clipboard with no picture on it answers `None` and nothing
/// happens, which is what makes it safe to ask after an ordinary text paste as well.
fn pasting(ui: &egui::Ui, in_the_composer: bool) -> bool {
    // **This pane's own field, not any field.** `text_box_has_the_keyboard` answers whether *some* box in
    // the window has the keys, and with this pane showing beside a focused explorer filter that was enough
    // to attach the clipboard's picture to a conversation nobody was typing into. The composer says whether
    // it has them — see `agent_chat::PaneState::prompt_focused`.
    if !in_the_composer {
        return false;
    }
    ui.ctx().input(|input| is_a_paste_chord(&input.events))
}

/// Whether this frame's events carry the paste chord as Quill can actually see it.
///
/// Its own function so a test can hold the two event shapes side by side: the one that was being
/// watched for and never arrives, and the one that does.
fn is_a_paste_chord(events: &[egui::Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. }
                if modifiers.command
        )
    })
}

/// Whether a file the window manager dropped belongs to the pane at `area`.
///
/// `None` means the platform reported no pointer through the drag, which Windows does not: a file is
/// carried over a window through OLE and no cursor movement is sent at all, so `egui` can still be
/// holding a position from before the drag started. Nothing else in Quill reads `dropped_files`, so a
/// drop nobody can place belongs here rather than nowhere.
fn belongs_here(pointer: Option<Pos2>, area: Rect) -> bool {
    pointer.is_none_or(|at| area.contains(at))
}

/// Every picture the window manager dropped on this pane this frame.
///
/// `egui` collects what was dropped and hands over a path; a file that is not a picture is left
/// alone rather than refused, because a drag that landed on the wrong pane should do nothing rather
/// than say something.
///
/// **`area` is the pane's own rectangle, taken before anything was drawn in it.** It used to ask the `Ui`
/// for what was left, which is a different rectangle by the time the composer has been laid out.
///
/// **A drop with no pointer is this pane's.** Windows carries a file over a window through OLE, which
/// reports no cursor movement at all, so `egui` can be holding a position from before the drag began — and
/// gating on it silently threw the picture away. Nothing else in Quill reads `dropped_files`, so a drop
/// whose position is not known belongs here rather than nowhere.
///
/// **`hover_pos` alone, and not `latest_pos` behind it.** The zoom asks for both because a wheel event
/// arrives with no pointer at all and the *last place it was seen* is still the honest answer. A drop is the
/// opposite case: `latest_pos` during an OLE drag is a position from before the drag began, so falling back
/// to it is falling back to a stale answer that reads as a confident one — which threw the picture away
/// exactly where this was meant to stop it. Found by the `task-1771` review.
fn dropped_pictures(ui: &egui::Ui, area: Rect) -> Vec<std::path::PathBuf> {
    let pointer = ui.ctx().input(|input| input.pointer.hover_pos());
    if !belongs_here(pointer, area) {
        return Vec::new();
    }
    ui.ctx().input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|dropped| dropped.path().to_path_buf())
            .filter(|path| {
                matches!(
                    path.extension()
                        .and_then(|kind| kind.to_str())
                        .map(str::to_lowercase)
                        .as_deref(),
                    Some("png" | "jpg" | "jpeg" | "gif" | "webp")
                )
            })
            .collect()
    })
}

/// The platform's own picture picker.
///
/// `rfd`, which is what `File -> Open` already uses, so a plugin does not bring a second file dialog.
/// The filter is the four kinds both APIs accept, because offering a `.txt` here would offer
/// something that is refused by the server rather than by Quill.
fn pick_a_picture() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp"])
        .set_title("Attach a picture")
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `task-1771`: pasting a picture into the composer had never worked, and this is why.
    #[test]
    fn a_paste_is_seen_on_the_key_going_up_and_never_as_an_empty_paste_event() {
        let chord = |pressed: bool, command: bool| egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers { command, ..Default::default() },
        };
        // What this used to watch for. `egui-winit` pushes `Event::Paste` only when the clipboard held
        // **text**, so with a picture on it the frame carries nothing at all — and the condition below is
        // one no window ever satisfies.
        assert!(!is_a_paste_chord(&[egui::Event::Paste(String::new())]));
        // The press is swallowed by the same early return that reads the clipboard.
        assert!(!is_a_paste_chord(&[chord(true, true)]));
        // The release is not, and the modifier is still down on it. This is the one report that arrives.
        assert!(is_a_paste_chord(&[chord(false, true)]));
        // A `V` with nothing held is a letter somebody typed.
        assert!(!is_a_paste_chord(&[chord(false, false)]));
    }

    /// A drop the platform cannot place is this pane's, because nothing else in Quill wants one.
    #[test]
    fn a_drop_with_no_pointer_still_lands_on_the_pane() {
        let area = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(400.0, 600.0));
        assert!(belongs_here(Some(Pos2::new(200.0, 300.0)), area), "over the pane");
        assert!(!belongs_here(Some(Pos2::new(20.0, 300.0)), area), "over something else");
        // Windows carries a file through OLE and sends no cursor movement, so the last position `egui`
        // holds can be from before the drag began. Refusing there threw the picture away silently.
        assert!(belongs_here(None, area), "nowhere in particular");
    }

    #[test]
    fn the_starter_chips_are_about_what_quill_is() {
        // Four, which is what the page this is modelled on has, and each is a thing somebody asks an
        // editor rather than a thing somebody asks a general chat.
        assert_eq!(STARTERS.len(), 4);
        for prompt in STARTERS {
            assert!(!prompt.is_empty());
            assert!(prompt.chars().count() < 30, "{prompt} is too long for a chip");
        }
    }
}
