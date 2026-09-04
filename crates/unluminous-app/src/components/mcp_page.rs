//! `Settings -> Tools -> MCP`: how an AI agent is given Unluminous.
//!
//! Built out of the Settings window's own furniture — `settings_dialog`'s breadcrumb, sections,
//! rows, notes, tick box and button — for the reason `plugins_page` records: a second page that
//! looks nearly the same is worse than one that looks the same.
//!
//! ## Install comes first, because it is what somebody opened the page for
//!
//! One button a client, and the label is what it will do: `Install for Claude Code`, or
//! `Remove from Claude Code` once it is there. Whether it is there is read when the page is opened
//! and again after a click rather than every frame, because it is a file on disk and a page that
//! reads one sixty times a second is a page that hesitates.
//!
//! The buttons write the **stdio** server: the agent launches `unluminous-cli mcp serve` itself, nothing
//! listens on a port, and the process lives exactly as long as the conversation. That is why the
//! server section under it is off by default and does not need to be touched for the buttons to
//! work.
//!
//! ## And the configuration is shown whether or not there is a button for it
//!
//! The block, verbatim, with a `Copy`: the JSON for anything that reads an `mcpServers` object, or
//! the TOML Codex reads. It is the answer for a client nobody has written a button for, and it is
//! also how somebody checks what a button just did.
//!
//! **One block at a time, chosen by two buttons**, rather than both at once. Two reasons, and the
//! first is the ordinary one: a page is a fixed rectangle and a settings page in Unluminous does not
//! scroll, so the two together ran off the bottom of the modal. The second is why the chooser is
//! two buttons rather than a dropdown — a dropdown is a popup, egui keeps at most one of those open
//! at a time, and this page already has one for the tool shape. It is the rule that turned the
//! three line spacings in the text options panel into three buttons.

use std::path::{Path, PathBuf};

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use unluminous_cli::mcp::install::{self, Client, Wanted};
use unluminous_cli::mcp::Shape;

use crate::components::controls;
use crate::components::settings_dialog::{breadcrumb, checkbox, label, note, row_at, section, wide_button};
use crate::services::mcp::State;
use crate::settings::{clamp_port, Page, Settings};
use crate::theme::{color, size};

/// How wide the configuration blocks are drawn.
const BLOCK_WIDTH: f32 = 560.0;

/// What the page is showing between frames.
///
/// It lives in the Settings window's state, so what a button did is still on the screen when the
/// page is left and come back to.
#[derive(Debug, Clone, Default)]
pub struct McpState {
    /// The port, as it is being typed. A field holds text rather than a number, because a person
    /// halfway through typing `7345` has typed `73`, and a number would have snapped to it.
    pub port: String,
    /// Whether each client already has Unluminous, read when the page opens and after every click.
    pub claude_installed: bool,
    pub codex_installed: bool,
    /// Whether it has been read at all yet.
    pub read: bool,
    /// What the last button did, for the line under it.
    pub said: Option<String>,
    /// Which client's configuration block is showing.
    pub showing: Option<Client>,
}

impl McpState {
    /// Read each client's configuration, which is what decides what the buttons say.
    pub fn refresh(&mut self, settings: &Settings, program: &Path) {
        let wanted = wanted_from(settings, program);
        self.claude_installed = install::installed(Client::Claude, &wanted).is_some();
        self.codex_installed = install::installed(Client::Codex, &wanted).is_some();
        self.read = true;
        if self.port.is_empty() {
            self.port = settings.mcp_port.to_string();
        }
    }
}

/// What the page asked for.
#[derive(Debug, Default, PartialEq)]
pub struct McpOutcome {
    /// A setting changed, so the window applies it and writes the settings file.
    pub changed: bool,
}

/// Draw the page into `area`.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    state: &mut McpState,
    settings: &mut Settings,
    running: &State,
    program: &Path,
) -> McpOutcome {
    let mut outcome = McpOutcome::default();
    if !state.read {
        state.refresh(settings, program);
    }
    let mut pen = breadcrumb(ui, area, Page::Mcp);

    pen = install_section(ui, area, pen, state, settings, program);
    pen = server_section(ui, area, pen + 6.0, state, settings, running, &mut outcome);
    configuration_section(ui, area, pen + 6.0, state, settings, program);
    outcome
}

/// The two buttons, and what the last one did.
fn install_section(
    ui: &mut egui::Ui,
    area: Rect,
    top: f32,
    state: &mut McpState,
    settings: &Settings,
    program: &Path,
) -> f32 {
    let mut pen = section(ui, area, top, "Install");
    pen = note(
        ui,
        area,
        pen,
        "Writes Unluminous into the agent's own configuration. The agent then launches Unluminous's server \
         itself over a pipe, so nothing listens on a port.",
    );
    let row = row_at(area, pen);
    let mut left = row.left();
    for (client, installed) in
        [(Client::Claude, state.claude_installed), (Client::Codex, state.codex_installed)]
    {
        let name = if installed {
            format!("Remove from {}", client.title())
        } else {
            format!("Install for {}", client.title())
        };
        let button = Rect::from_min_size(Pos2::new(left, row.top()), Vec2::new(196.0, 28.0));
        if wide_button(ui, button, &name) {
            let wanted = wanted_from(settings, program);
            let done = if installed {
                install::remove(client, &wanted)
            } else {
                install::install(client, &wanted)
            };
            state.said = Some(match done {
                Ok(done) => done.message,
                Err(problem) => problem,
            });
            state.refresh(settings, program);
        }
        left = button.right() + 12.0;
    }
    pen += 32.0;
    match &state.said {
        Some(said) => note(ui, area, pen, said),
        None => note(
            ui,
            area,
            pen,
            "Restart the agent afterwards. In Claude Code, `/mcp` then lists Unluminous's tools.",
        ),
    }
}

/// The tick box, the port, the tool shape, and what is actually happening right now.
fn server_section(
    ui: &mut egui::Ui,
    area: Rect,
    top: f32,
    state: &mut McpState,
    settings: &mut Settings,
    running: &State,
    outcome: &mut McpOutcome,
) -> f32 {
    let mut pen = section(ui, area, top, "Server");
    let row = row_at(area, pen);
    let mut enabled = settings.mcp_enabled;
    if checkbox(ui, row, "Also serve over HTTP on this machine", &mut enabled) {
        settings.mcp_enabled = enabled;
        outcome.changed = true;
    }
    pen += 34.0;

    // The port and the tool shape share a row. Two rows of one control each is what this looked
    // like first, and it cost a line the page has not got: everything below is the thing somebody
    // came here to copy.
    let port_row = row_at(area, pen);
    label(ui, area, port_row, "Port:");
    let before = state.port.clone();
    crate::components::modal::field(
        ui,
        Rect::from_min_size(Pos2::new(area.left() + 70.0, port_row.top()), Vec2::new(84.0, 28.0)),
        "MCP port",
        &mut state.port,
    );
    if state.port != before {
        // A number is taken from the field only when it is one. Half a port number is what somebody
        // typing looks like, and snapping the setting to `73` on the way to `7345` would start a
        // listener on a port nobody asked for.
        if let Ok(port) = state.port.trim().parse::<f32>() {
            let port = clamp_port(port);
            if port != settings.mcp_port {
                settings.mcp_port = port;
                outcome.changed = true;
            }
        }
    }
    let shape_row = Rect::from_min_size(
        Pos2::new(port_row.left() + 200.0, port_row.top()),
        Vec2::new(port_row.width() - 200.0, port_row.height()),
    );
    label(ui, area, shape_row, "Tools:");
    if let Some(chosen) = controls::dropdown(
        ui,
        Rect::from_min_size(Pos2::new(shape_row.left() + 54.0, shape_row.top()), Vec2::new(200.0, 28.0)),
        shape_name(settings.mcp_tools),
        "MCP tool shape",
        None,
        |ui| {
            let mut chosen = None;
            for shape in [Shape::Grouped, Shape::Every] {
                if ui.selectable_label(settings.mcp_tools == shape, shape_name(shape)).clicked() {
                    chosen = Some(shape);
                }
            }
            chosen
        },
    ) {
        settings.mcp_tools = chosen;
        outcome.changed = true;
    }
    pen += 32.0;
    pen = note(
        ui,
        area,
        pen,
        &format!(
            "{} tools. One an area names every command Unluminous has for about a third of the context \
             one a command costs; one a command lets an agent permit a single tool by name.",
            unluminous_cli::mcp::tools::tools(settings.mcp_tools).len()
        ),
    );
    note(ui, area, pen, &running.message())
}

/// The two blocks, and a `Copy` for each.
fn configuration_section(
    ui: &mut egui::Ui,
    area: Rect,
    top: f32,
    state: &mut McpState,
    settings: &Settings,
    program: &Path,
) -> f32 {
    let mut pen = section(ui, area, top, "Configuration");
    pen = note(
        ui,
        area,
        pen,
        "For an agent with no button above. Paste it into that client's own configuration file.",
    );
    let showing = state.showing.unwrap_or(Client::Claude);
    let row = row_at(area, pen);
    let mut left = row.left();
    for client in Client::ALL {
        let button = Rect::from_min_size(Pos2::new(left, row.top()), Vec2::new(118.0, 26.0));
        if chooser(ui, button, client.title(), client == showing) {
            state.showing = Some(client);
        }
        left = button.right() + 8.0;
    }
    let wanted = wanted_from(settings, program);
    let copy = Rect::from_min_size(Pos2::new(row.right() - 60.0, row.top()), Vec2::new(60.0, 26.0));
    if wide_button(ui, copy, "Copy") {
        ui.ctx().copy_text(wanted.example(showing));
    }
    block(ui, area, pen + 32.0, showing.title(), &wanted.example(showing))
}

/// One of the two buttons that choose which block is showing, drawn the way a chosen control is
/// drawn everywhere else: the accent behind it, the ordinary control fill behind the other.
fn chooser(ui: &mut egui::Ui, area: Rect, name: &str, chosen: bool) -> bool {
    let response = ui.interact(area, ui.id().with(("mcp-chooser", name)), Sense::click());
    let fill = if chosen || response.hovered() { color::accent() } else { color::control() };
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        fill,
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        color::text_strong(),
    );
    painter.galley(area.center() - galley.size() / 2.0, galley, color::text_strong());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, name)
    });
    response.clicked()
}

/// One read-only block of configuration.
fn block(ui: &mut egui::Ui, area: Rect, top: f32, title: &str, text: &str) -> f32 {
    let mut pen = top;
    let painter = ui.painter_at(area);
    let galley = painter.layout(
        text.to_owned(),
        egui::FontId::monospace(10.5),
        color::text_control(),
        BLOCK_WIDTH - 24.0,
    );
    let frame = Rect::from_min_size(
        Pos2::new(area.left() + 24.0, pen),
        Vec2::new(BLOCK_WIDTH.min(area.width() - 48.0), galley.size().y + 16.0),
    );
    painter.rect(
        frame,
        CornerRadius::same(size::CONTROL_CORNER),
        color::field(),
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
    painter.galley(frame.min + Vec2::new(12.0, 8.0), galley, color::text_control());
    // Named so a screenshot test can find it, and so somebody reading the window out loud is told
    // what the block is rather than being read the whole of it.
    let response = ui.interact(frame, ui.id().with(("mcp-block", title)), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            ui.is_enabled(),
            format!("{title} configuration"),
        )
    });
    pen += frame.height() + 10.0;
    pen
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Grouped => "One tool an area",
        Shape::Every => "One tool a command",
    }
}

/// What the buttons and the blocks describe.
///
/// The stdio transport, always. The HTTP endpoint is a thing somebody switched on for a client that
/// wants a URL, and it is not what an install button should quietly write: a configuration naming a
/// port would stop working the moment the tick box above it was cleared.
fn wanted_from(settings: &Settings, program: &Path) -> Wanted {
    Wanted { port: settings.mcp_port, program: PathBuf::from(program), ..Wanted::default() }
}
