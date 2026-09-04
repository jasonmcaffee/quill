//! The composer: the pill of tools, what has been used, the attachments and the prompt.
//!
//! `ChatComposer.module.css`, `ChatComposerToolbar.module.css`, `ChatTool.module.css` and
//! `PromptInput.module.css` are what this is measured against. The pill is a **pressed** well holding
//! round buttons that are flat until they are switched on, at which point each is **raised** and
//! wears its own accent — which is `ChatTool`'s whole design, and the reason a toolbar of six reads
//! as a row of states rather than as a row of buttons.
//!
//! ## The send button is the stop button
//!
//! While an answer is arriving there is nothing to send and something to stop, so the one disc at the
//! end of the prompt is whichever applies. That is `PromptInput`'s own `onStopButtonPress`, and it is
//! Unluminous's rule about a control that cannot apply being absent rather than present and refusing.
//!
//! ## There is no context meter, and that is deliberate
//!
//! The page this is modelled on draws a bar of how much of the model's context has been used, and it
//! can: its own server knows the window the model was loaded with. Unluminous does not — a URL and a model
//! name say nothing about a context length — so a bar here would be a fraction of a number nobody
//! measured. What is drawn instead is what the server really reported: the tokens in and the tokens
//! out. A control that cannot apply is absent, which is the rule the `F` button already keeps.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::Act;
use crate::services::agent_chat::Parts;
use crate::services::plugin_ui::Look;
use crate::services::vello_canvas::{Fill, Lift};
use crate::theme::icon;

/// The pill of tools, and one round button in it.
const PILL: f32 = 28.0;
const TOOL: f32 = 22.0;
/// The row that says what has been used.
const USED: f32 = 14.0;
/// A thumbnail of an attached picture, and the row it sits in.
const THUMB: f32 = 38.0;
/// The prompt well when there is one line in it, and the most it grows to.
///
/// **Sixty-eight rather than forty-two**, which is the reference's own: at forty-two the field was
/// shallower than its own send button and the whole composer read as cramped, which is the first
/// thing a review of the picture said about it.
const PROMPT: f32 = 68.0;
const PROMPT_ROWS: usize = 6;
/// The disc at the end of the prompt.
const SEND: f32 = 32.0;
/// Between the composer's rows.
const GAP: f32 = 8.0;

/// How tall the composer is, which the pane measures back from its own bottom.
pub fn height(parts: &Parts<'_>, look: &Look<'_>, width: f32) -> f32 {
    let scale = look.scale();
    let mut total = PILL + GAP + prompt_height(parts, look, width);
    if parts.session.chat.usage.total() > 0 || parts.problem.is_some() {
        total += USED + GAP * 0.5;
    }
    if !parts.attachments.is_empty() {
        total += THUMB + GAP * 0.5;
    }
    total * scale
}

/// How tall the prompt well is, in unscaled points: one line, grown by what has been typed.
fn prompt_height(parts: &Parts<'_>, look: &Look<'_>, width: f32) -> f32 {
    let per_line = look.font_size * 1.45;
    // The lines it takes, counting a wrap at roughly the width of the field — enough to grow the
    // well as somebody types a paragraph, which is what the page this copies does.
    let across = ((width - SEND - 40.0) / (look.font_size * 0.48)).max(8.0);
    let lines: usize = parts
        .draft
        .lines()
        .map(|line| ((line.chars().count() as f32 / across).ceil() as usize).max(1))
        .sum::<usize>()
        .clamp(1, PROMPT_ROWS);
    PROMPT + (lines.saturating_sub(1) as f32) * per_line
}

/// Draw the composer and say what was pressed.
pub fn show(mut parts: Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let mut pen = area.top();

    acts.extend(pill(
        &parts,
        ui,
        look,
        Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), PILL * scale)),
    ));
    pen += (PILL + GAP) * scale;

    if parts.session.chat.usage.total() > 0 || parts.problem.is_some() {
        used(
            &parts,
            ui,
            look,
            Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), USED * scale)),
        );
        pen += (USED + GAP * 0.5) * scale;
    }

    if !parts.attachments.is_empty() {
        acts.extend(thumbnails(
            &mut parts,
            ui,
            look,
            Rect::from_min_size(
                Pos2::new(area.left(), pen),
                Vec2::new(area.width(), THUMB * scale),
            ),
        ));
        pen += (THUMB + GAP * 0.5) * scale;
    }

    let well = Rect::from_min_max(Pos2::new(area.left(), pen), area.max);
    acts.extend(prompt(parts, ui, look, well));
    acts
}

/// The pressed pill of round buttons.
fn pill(parts: &Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    // Three, and each is a **state** rather than a command: whether the model may drive the window, a
    // picture waiting to go up with the next message, and whether the answer arrives a token at a
    // time. That is `ChatTool`'s own design — a pill of states reads at a glance where a pill of
    // buttons does not. Anything that is not a state is elsewhere: new and the history are in the
    // header, and stop is the send button. The history had a second button here and it was taken
    // away, because two controls doing one thing in one pane is one too many.
    //
    // **Two of the three are absent when the row is a command-line agent**, which is the
    // absent-control rule rather than tidiness: `claude` and `codex` bring their own tools, so
    // handing them Unluminous's would be offering a switch that does nothing, and they always stream, so
    // a switch that turned it off would be a switch that lies. What is left is the attachment, which
    // means the same thing either way.
    let an_agent = parts
        .configuration
        .provider()
        .is_some_and(|one| one.is_a_program());
    let mut tools: Vec<(&str, fn(&egui::Painter, Pos2, Color32), bool, Color32, Act)> = Vec::new();
    if !an_agent {
        tools.push((
            "Unluminous tools",
            icon::terminal,
            parts.configuration.tools,
            crate::theme::color::agent(),
            Act::ToggleTools,
        ));
    }
    tools.push((
        "Attach a picture",
        icon::image,
        !parts.attachments.is_empty(),
        look.palette.board_accent,
        Act::Attach,
    ));
    if !an_agent {
        tools.push((
            "Stream",
            icon::run,
            parts.configuration.stream,
            look.palette.attached,
            Act::ToggleStream,
        ));
    }
    let buttons = tools.len();
    let width = (TOOL * buttons as f32 + 4.0 * (buttons as f32 + 1.0)) * scale;
    let pill = Rect::from_center_size(
        Pos2::new(area.center().x, area.center().y),
        Vec2::new(width, PILL * scale),
    );
    if look.chrome.is_recording() {
        look.chrome
            .sunken(pill, PILL * scale / 2.0, look.palette.board_well, Lift::Small);
    } else {
        ui.painter().rect(
            pill,
            CornerRadius::same((PILL * scale / 2.0) as u8),
            look.ground(look.palette.board_well),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let mut centre = Pos2::new(pill.left() + (4.0 + TOOL / 2.0) * scale, pill.center().y);
    for (name, drawing, on, accent, act) in tools {
        let at = Rect::from_center_size(centre, Vec2::splat(TOOL * scale));
        let response = ui.interact(at, ui.id().with(("agent-chat-tool-button", name)), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.to_owned()));
        if on {
            if look.chrome.is_recording() {
                look.chrome.raised(
                    at,
                    TOOL * scale / 2.0,
                    Fill::Solid(look.palette.board_card),
                    Lift::Small,
                );
            } else {
                ui.painter()
                    .circle_filled(centre, TOOL * scale / 2.0, look.palette.board_card);
            }
        }
        let tint = match (on, response.hovered()) {
            (true, _) => accent,
            (false, true) => look.palette.text_strong,
            (false, false) => look.palette.text_dim,
        };
        drawing(&ui.painter_at(area), centre, tint);
        if response.clicked() {
            acts.push(act);
        }
        centre.x += (TOOL + 4.0) * scale;
    }
    acts
}

/// What the server said this conversation has cost, and whatever went wrong before a request.
fn used(parts: &Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) {
    let painter = ui.painter_at(area);
    let (said, tint) = match parts.problem {
        Some(problem) => (problem.to_owned(), crate::theme::color::close()),
        None => (
            format!(
                "in {} · out {}",
                thousands(parts.session.chat.usage.input),
                thousands(parts.session.chat.usage.output)
            ),
            look.palette.text_faint,
        ),
    };
    let font = egui::FontId::monospace(look.font_size * 0.68);
    let galley = painter.layout(said, font, tint, area.width());
    let at = Pos2::new(
        area.center().x - galley.size().x.min(area.width()) / 2.0,
        area.top(),
    );
    painter.galley(at, galley, tint);
}

/// A number a person reads: `18k` rather than `18342`.
fn thousands(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        _ => format!("{:.1}k", count as f32 / 1000.0),
    }
}

/// The pictures waiting to go up, each with a cross that takes it off again.
fn thumbnails(parts: &mut Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let mut left = area.left() + 2.0 * scale;
    // Copied out so the pictures can be uploaded into the state while the list is walked; both are
    // borrows of different fields, which is what makes this legal at all.
    let attachments: &[crate::services::agent_chat::Attachment] = parts.attachments;
    for attachment in attachments {
        let at = Rect::from_min_size(Pos2::new(left, area.top()), Vec2::splat(THUMB * scale));
        if at.right() > area.right() {
            break;
        }
        let key = format!("attachment-{}", attachment.id);
        if !parts.state.pictures.contains_key(&key) {
            if let Ok(image) = crate::services::picture::decode_bytes(&attachment.bytes) {
                let texture = crate::services::picture::upload(
                    ui.ctx(),
                    key.clone(),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                parts.state.pictures.insert(key.clone(), texture);
            }
        }
        if look.chrome.is_recording() {
            look.chrome
                .raised(at, 8.0 * scale, Fill::Solid(look.palette.board_well), Lift::Small);
        } else {
            ui.painter().rect_filled(
                at,
                CornerRadius::same((8.0 * scale) as u8),
                look.palette.board_well,
            );
        }
        match parts.state.pictures.get(&key) {
            Some(texture) => {
                ui.painter_at(area).image(
                    texture.id(),
                    at.shrink(2.0),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            // A picture that will not decode still says it is attached, because it will still be
            // sent — the server is the one that decides whether it can read it.
            None => icon::image(&ui.painter_at(area), at.center(), look.palette.text_dim),
        }
        let cross = Rect::from_center_size(
            Pos2::new(at.right() - 2.0 * scale, at.top() + 2.0 * scale),
            Vec2::splat(14.0 * scale),
        );
        if crate::components::controls::icon_button(
            ui,
            cross,
            &format!("Take off {}", attachment.name),
            icon::cross,
        ) {
            acts.push(Act::Detach(attachment.id));
        }
        left += (THUMB + 6.0) * scale;
    }
    acts
}

/// The prompt well: the field, and the disc that sends or stops.
fn prompt(parts: Parts<'_>, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let radius = (area.height() / 2.0).min(18.0 * scale);
    // **Deeper than anything else in the pane**, which is `PromptInput.module.css`'s `--e-pressed`:
    // the field somebody types into is the one thing pressed furthest into the page.
    if look.chrome.is_recording() {
        look.chrome
            .sunken(area, radius, look.palette.board_well, Lift::Medium);
    } else {
        ui.painter().rect(
            area,
            CornerRadius::same(radius as u8),
            look.ground(look.palette.board_well),
            Stroke::new(1.0, look.palette.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let busy = parts.session.is_busy();
    let disc = Rect::from_center_size(
        Pos2::new(
            area.right() - (SEND / 2.0 + 5.0) * scale,
            area.bottom() - (SEND / 2.0 + 5.0) * scale,
        ),
        Vec2::splat(SEND * scale),
    );
    let field = Rect::from_min_max(
        Pos2::new(area.left() + 12.0 * scale, area.top() + 6.0 * scale),
        Pos2::new(disc.left() - 8.0 * scale, area.bottom() - 6.0 * scale),
    );

    parts.state.prompt_focused = false;
    if field.width() > 30.0 {
        let rows = ((field.height() / (look.font_size * 1.45)).floor() as usize).max(1);
        let prompt_id = ui.id().with("agent-chat-prompt");
        let response = ui.put(
            crate::components::controls::field_takes_the_whole_rectangle(ui, field, 0.0, prompt_id),
            egui::TextEdit::multiline(parts.draft)
                .id(prompt_id)
                .frame(egui::Frame::NONE)
                .hint_text(egui::RichText::new("Ask anything…").color(look.palette.text_faint))
                .desired_width(field.width())
                .desired_rows(rows)
                .font(egui::FontId::proportional(look.font_size * 0.9))
                .text_color(look.palette.text),
        );
        // Named, because every control in Unluminous has a plain name and a test finds one by it. Its hint
        // text is not a name: it is what the field says when it is empty.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Message"));
        // Recorded so the paste knows whose key press it is reading — see `PaneState::prompt_focused`.
        parts.state.prompt_focused = response.has_focus();
        // **Enter sends and Shift+Enter is a new line**, which is what the page this copies does and
        // what everybody expects of a chat.
        //
        // The modifiers are compared **for real** rather than through `consume_key`, which matches by
        // `Modifiers::matches_logically`: that only asks whether the modifiers the *pattern* names are
        // held, so a pattern of `NONE` takes `Shift+Enter` too — the trap `task-1678` recorded and
        // `task-1682` recorded again. And on Windows `Ctrl+Enter` arrives with **both** `ctrl` and
        // `command` set, which `is_none` excludes and an equality test against `Modifiers::NONE` would
        // not.
        //
        // The field has already put a new line in the draft by the time this runs, because `TextEdit`
        // reads the frame's events first. That costs nothing: `AgentChat::send` trims the end of what
        // it is given, so the line break the field added never reaches the message.
        if response.has_focus() && !busy {
            let send = ui.input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.is_none());
            if send {
                acts.push(Act::Send);
            }
        }
    }

    // The one disc: send while there is something to send, stop while an answer is arriving.
    //
    // **It senses a click only when it can do something**, which is Unluminous's rule about a control that
    // cannot apply. It is still *drawn*, because a button that vanished as the field emptied would
    // make the field jump about while somebody was typing in it — absent here means inert rather than
    // gone, and it says so by being quiet.
    let ready = busy || !parts.draft.trim().is_empty() || !parts.attachments.is_empty();
    let sense = match ready {
        true => Sense::click(),
        false => Sense::hover(),
    };
    let response = ui.interact(disc, ui.id().with("agent-chat-send"), sense);
    let name = match busy {
        true => "Stop answering",
        false => "Send",
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ready, name.to_owned()));
    let (start, end) = match (busy, ready) {
        (true, _) => (
            crate::theme::color::close(),
            crate::theme::color::close().gamma_multiply(0.75),
        ),
        (false, true) => (
            look.palette.board_accent,
            super::darken(look.palette.board_accent, 0.15),
        ),
        // Nothing to send: the disc is there but quiet, because a button that vanished as the field
        // emptied would make the field jump about while somebody was typing in it.
        (false, false) => (look.palette.board_card, look.palette.board_card),
    };
    if look.chrome.is_recording() {
        if ready {
            // The blue glow under the primary button, which is the reference's own
            // `4px 4px 12px rgba(29,79,219,0.35)`.
            look.chrome
                .glow(disc, disc.width() / 2.0, start.gamma_multiply(0.45), 7.0 * scale);
        }
        look.chrome.disc(
            disc.center(),
            disc.width() / 2.0,
            Fill::diagonal(disc, start, end),
        );
    } else {
        ui.painter()
            .circle_filled(disc.center(), disc.width() / 2.0, start);
    }
    let tint = match ready {
        // The palette's own white rather than `Color32::WHITE`: the palette is closed, and a colour
        // written out here is a colour no theme can reach.
        true => look.palette.text_strong,
        false => look.palette.text_faint,
    };
    match busy {
        true => icon::stop(&ui.painter_at(area), disc.center(), tint),
        false => send_arrow(&ui.painter_at(area), disc.center(), tint, scale),
    }
    if response.clicked() {
        acts.push(match busy {
            true => Act::Stop,
            false => Act::Send,
        });
    }
    acts
}

/// The arrow on the send button: a shaft and two strokes, drawn rather than lettered.
fn send_arrow(painter: &egui::Painter, centre: Pos2, tint: Color32, scale: f32) {
    let half = 5.0 * scale;
    let stroke = Stroke::new(1.8 * scale, tint);
    painter.line_segment(
        [
            Pos2::new(centre.x, centre.y + half),
            Pos2::new(centre.x, centre.y - half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(centre.x - half * 0.75, centre.y - half * 0.2),
            Pos2::new(centre.x, centre.y - half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(centre.x + half * 0.75, centre.y - half * 0.2),
            Pos2::new(centre.x, centre.y - half),
        ],
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_of_tokens_is_written_the_way_a_person_reads_one() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(18_342), "18.3k");
    }
}
