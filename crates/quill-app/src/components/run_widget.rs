//! The run widget at the right of the title bar: the chosen configuration's name, a play button, a
//! bug button, and a stop square while it runs.
//!
//! `tasks/task-1683-run-configurations-tdd.md` §6.2. It sits at the **right hand end of the bar**,
//! in front of the window buttons and behind the text tools, **inside the height the title bar
//! already has** — the bar's height never changes, so a widget that appeared and disappeared with
//! the project would not move the tabs or the editing area by a pixel.
//! `components::title_bar` decides where it goes, through
//! [`crate::components::title_bar::run_rect`], exactly as it decides where the text tools go.
//!
//! It was in front of the tools until `task-1693`, and moved because the tools are as wide as the
//! open file needs them to be — so the play button slid along the bar every time the tab changed.
//!
//! Three parts, and each is absent when it cannot apply, which is Quill's rule:
//!
//! - **The name**, with a chevron, opening the flyout: every configuration — permanent first,
//!   temporaries and the detectors' suggestions after them in the quiet colour — each row with a
//!   small play icon and a green dot when it is running; then `Run Current File` when the open
//!   file's language names one; then `Edit Configurations...`. One flyout and no submenus, which is
//!   the rule `controls::flyout` records: egui keeps at most one popup open at a time.
//! - **Play**, running the selected configuration. With none selected it opens the dialog instead,
//!   which is what `Add Configuration...` means without a second control meaning it.
//! - **Debug**, the bug beside it, running the same configuration under its debugger — `task-1692`,
//!   and IntelliJ's own pair in IntelliJ's own order. It is present when the configuration the play
//!   button would start **resolves to a debugger at all**, which `QuillApp::run_widget_state` works
//!   out: a Rust or a Node project therefore always shows both buttons, and a vault of Markdown and
//!   CSS shows one, because there is nothing there to step through and never will be. That is
//!   Quill's rule for a control that cannot apply, asked of the thing the button acts on rather than
//!   of whichever tab happens to be focused — a button that came and went as tabs were switched
//!   would be worse than either answer.
//! - **Stop**, drawn **only while the selected configuration runs** — a control absent when it
//!   cannot apply.
//!
//! With no configurations and no runnable file the widget is the play button alone: present,
//! because the way to discover the feature has to be visible, and small, because it is not yet in
//! use.
//!
//! Nothing here changes anything. Every press is an [`Action`], so the widget, the `Run` menu, the
//! rail and the keyboard all go down the one path in `QuillApp::run_action`.

use egui::{CornerRadius, Pos2, Rect, Sense, Vec2};

use crate::app::actions::{Action, DebugAction, RunAction};
use crate::components::controls;
use crate::services::run_configurations::Origin;
use crate::theme::{color, icon, size};

/// How big one of the widget's square buttons is. The text tools' button, because they sit beside
/// each other in the same bar and two nearly-equal sizes would read as a mistake.
pub const BUTTON: f32 = crate::components::text_tools::BUTTON;

/// How many characters of a configuration's name are shown before it is cut short.
///
/// Sixteen: enough for `Dev server` and `npm run build` whole, and short enough that a name
/// somebody pasted cannot take the title bar. The whole name is in the flyout and in the hover.
pub const NAME_LIMIT: usize = 16;

/// How wide the flyout is.
const PANEL: f32 = 260.0;
/// How tall one row of it is. A menu row, because that is what it is.
const ROW: f32 = 24.0;

/// One row of the flyout: a configuration the project offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// Where it came from, which is what decides the colour it is drawn in: a permanent is
    /// ordinary text and a temporary or a suggestion is the quiet colour, the way an occurrence
    /// inside a comment is listed in the references modal.
    pub origin: Origin,
    /// True when this configuration has a run going.
    pub running: bool,
}

/// What the widget needs to know to draw itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetState {
    /// The configuration the widget has chosen, if one is.
    pub selected: Option<String>,
    /// Every configuration the project offers, in the order they are listed.
    pub rows: Vec<Row>,
    /// True when the chosen configuration is running, which is what draws the stop square.
    pub running: bool,
    /// True when something here could be debugged, which is what draws the bug button. Worked out by
    /// the window from the configuration the play button would start, falling back to the open
    /// file's language — see [`crate::app::QuillApp::adapter_for`].
    pub debuggable: bool,
    /// The name `Run Current File` would give its temporary — the open file's name — when the
    /// file's language says how one file of it is run. `None` puts the entry away altogether.
    pub current_file: Option<String>,
}

impl WidgetState {
    /// True when there is a list worth opening, which is what decides whether the name and its
    /// chevron are drawn at all.
    fn has_a_list(&self) -> bool {
        !self.rows.is_empty() || self.current_file.is_some()
    }

    /// What the name button says: the chosen configuration, cut short, or a word standing in for
    /// one when nothing has been chosen yet.
    fn label(&self) -> String {
        match &self.selected {
            Some(name) => elide(name),
            None => "Run".to_owned(),
        }
    }
}

/// A name cut short at [`NAME_LIMIT`] with an ellipsis, or left alone when it fits.
fn elide(name: &str) -> String {
    if name.chars().count() <= NAME_LIMIT {
        return name.to_owned();
    }
    let kept: String = name.chars().take(NAME_LIMIT - 1).collect();
    format!("{kept}\u{2026}")
}

/// How much room the widget wants, which the title bar leaves clear at its right hand end.
///
/// Worked out from the number of characters rather than by measuring, because the title bar has to
/// know before anything is drawn — the same arithmetic `modal::footer` already does for a button's
/// width, and for the same reason.
pub fn width(state: &WidgetState) -> f32 {
    let mut width = BUTTON;
    if state.has_a_list() {
        width += name_width(&state.label()) + 4.0;
    }
    if state.debuggable {
        width += 2.0 + BUTTON;
    }
    if state.running {
        width += 2.0 + BUTTON;
    }
    width
}

/// How wide the name button is for a label of this length: the text, the chevron and the padding
/// either side.
fn name_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 6.8 + 34.0).round()
}

/// Draw the widget into `area` and report what was pressed.
pub fn show(ui: &mut egui::Ui, area: Rect, state: &WidgetState) -> Option<Action> {
    let mut chosen: Option<Action> = None;
    let middle = area.center().y;
    let mut pen = area.left();

    if state.has_a_list() {
        let label = state.label();
        let button = Rect::from_min_size(
            Pos2::new(pen, middle - BUTTON / 2.0),
            Vec2::new(name_width(&label), BUTTON),
        );
        if let Some(action) =
            controls::labelled_flyout(ui, button, "Choose a run configuration", &label, PANEL, |panel| {
                flyout(panel, state)
            })
        {
            chosen = action;
        }
        pen = button.right() + 4.0;
    }

    // Play. With nothing chosen it opens the dialog, which is what `Add Configuration...` means
    // without a second control meaning it.
    let play = Rect::from_min_size(Pos2::new(pen, middle - BUTTON / 2.0), Vec2::splat(BUTTON));
    let name = match &state.selected {
        Some(_) => "Run the selected configuration",
        None => "Add a run configuration",
    };
    if square_button(ui, play, name, icon::run, state.selected.is_some()) {
        chosen = Some(match state.selected {
            Some(_) => Action::Run(RunAction::Start(None)),
            None => Action::Run(RunAction::Edit),
        });
    }
    pen = play.right() + 2.0;

    // Debug, beside it, which is IntelliJ's pair and IntelliJ's order. Same configuration, same
    // command, under a debugger — so with nothing chosen it opens the dialog, exactly as play does.
    if state.debuggable {
        let debug = Rect::from_min_size(Pos2::new(pen, middle - BUTTON / 2.0), Vec2::splat(BUTTON));
        let name = match &state.selected {
            Some(_) => "Debug the selected configuration",
            None => "Add a run configuration to debug",
        };
        // Tinted on the same rule as the play triangle beside it — the colour means "this starts
        // something", so the pair reads as a pair.
        if square_button(ui, debug, name, icon::bug, state.selected.is_some()) {
            chosen = Some(match state.selected {
                Some(_) => Action::Debug(DebugAction::Start(None)),
                None => Action::Run(RunAction::Edit),
            });
        }
        pen = debug.right() + 2.0;
    }

    // Stop, drawn only while it can apply — Quill's rule for a control that cannot.
    if state.running {
        let stop = Rect::from_min_size(Pos2::new(pen, middle - BUTTON / 2.0), Vec2::splat(BUTTON));
        if square_button(ui, stop, "Stop the selected configuration", icon::stop, false) {
            chosen = Some(Action::Run(RunAction::Stop(None)));
        }
    }

    chosen
}

/// One of the widget's square buttons: hovered fill, a drawn icon, a plain name.
///
/// `green` tints the icon with the colour that means "this starts something", which is IntelliJ's
/// own colour for the same button and is `theme::color::git_added()` here — the palette is closed,
/// and that is the green in it.
fn square_button(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    draw: fn(&egui::Painter, Pos2, egui::Color32),
    green: bool,
) -> bool {
    let response = ui
        .interact(area, ui.id().with(("run-widget", name)), Sense::click())
        .on_hover_text(name);
    if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::control());
    }
    let tint = if green { color::git_added() } else { color::text_control() };
    draw(ui.painter(), area.center(), tint);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response.clicked()
}

/// The flyout: the configurations, then `Run Current File`, then `Edit Configurations...`.
fn flyout(ui: &mut egui::Ui, state: &WidgetState) -> Option<Action> {
    let mut chosen: Option<Action> = None;
    for row in &state.rows {
        if let Some(action) = configuration_row(ui, row, state.selected.as_deref() == Some(&row.name))
        {
            chosen = Some(action);
        }
    }
    if state.rows.is_empty() {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("  No run configurations yet.").size(11.5).color(color::text_faint()),
        );
        ui.add_space(4.0);
    }
    // `Run Current File`, when the open file's language names a command. Absent otherwise, which is
    // the rule `file_kind` already applies to the three code-navigation entries.
    if let Some(file) = &state.current_file {
        ui.separator();
        if controls::menu_row(ui, &format!("Run {file}"), "", true, false, 0.0) {
            chosen = Some(Action::Run(RunAction::CurrentFile));
        }
    }
    ui.separator();
    if controls::menu_row(ui, "Edit Configurations...", "", true, false, 0.0) {
        chosen = Some(Action::Run(RunAction::Edit));
    }
    chosen
}

/// One configuration in the flyout: a state dot, the name, and a play icon that runs it.
///
/// Choosing the row **selects** it and its play icon **runs** it, which is the widget's whole
/// grammar and is IntelliJ's: the list is for picking what the play button at the top will do, and
/// the icon beside a row is for running that one without changing the choice.
fn configuration_row(ui: &mut egui::Ui, row: &Row, selected: bool) -> Option<Action> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let play = Rect::from_center_size(Pos2::new(rect.right() - 12.0, rect.center().y), Vec2::splat(18.0));
    let play_response =
        ui.interact(play, ui.id().with(("run-row-play", &row.name)), Sense::click());
    if response.hovered() || play_response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), color::selected_row());
    }
    let painter = ui.painter();
    let mut left = rect.left() + 8.0;
    if row.running {
        icon::state_dot(painter, Pos2::new(left + 4.0, rect.center().y), color::git_added());
    }
    left += 14.0;
    // A temporary and a suggestion are drawn in the quiet colour, the way an occurrence inside a
    // comment is listed in the references modal: they are offered rather than kept.
    let tint = match row.origin {
        Origin::Permanent => color::text_control(),
        Origin::Temporary | Origin::Suggested => color::text_dim(),
    };
    let galley = painter.layout_no_wrap(row.name.clone(), egui::FontId::proportional(12.5), tint);
    painter.galley(Pos2::new(left, rect.center().y - galley.size().y / 2.0), galley, tint);
    // Green, and with a pill of its own when the pointer is on it. A small grey triangle at the
    // right hand end of a menu row is what every menu in the world draws to mean "this opens a
    // submenu", and this one means the opposite: it is a button that runs the row it is on.
    if play_response.hovered() {
        painter.rect_filled(play, CornerRadius::same(4), color::control());
    }
    icon::run_scaled(painter, play.center(), color::git_added(), 0.78);
    let name = row.name.clone();
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name.clone())
    });
    let run_name = format!("Run {}", row.name);
    play_response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, run_name.clone())
    });
    if play_response.clicked() {
        return Some(Action::Run(RunAction::Start(Some(row.name.clone()))));
    }
    if response.clicked() {
        return Some(Action::Run(RunAction::Select(row.name.clone())));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, origin: Origin, running: bool) -> Row {
        Row { name: name.to_owned(), origin, running }
    }

    #[test]
    fn a_long_name_is_cut_short_and_a_short_one_is_left_alone() {
        assert_eq!(elide("Dev server"), "Dev server");
        assert_eq!(elide("npm run build:production"), "npm run build:p\u{2026}");
        assert_eq!(elide(&"x".repeat(NAME_LIMIT)), "x".repeat(NAME_LIMIT), "exactly the limit fits");
    }

    #[test]
    fn with_nothing_at_all_the_widget_is_the_play_button_alone() {
        // Present, because the way to discover the feature has to be visible; small, because it is
        // not yet in use.
        let state = WidgetState::default();
        assert_eq!(width(&state), BUTTON);
        assert!(!state.has_a_list());
    }

    #[test]
    fn the_name_appears_as_soon_as_there_is_something_to_list() {
        let suggested = WidgetState {
            rows: vec![row("cargo run", Origin::Suggested, false)],
            ..WidgetState::default()
        };
        assert!(suggested.has_a_list());
        assert!(width(&suggested) > BUTTON);
        // And a runnable file is enough on its own, because `Run Current File` is a row.
        let file = WidgetState { current_file: Some("server.js".to_owned()), ..WidgetState::default() };
        assert!(file.has_a_list());
    }

    /// The pair `task-1692` asks for: the bug button is one more button's worth of title bar, and it
    /// is there exactly when there is something it could debug.
    #[test]
    fn the_widget_grows_by_one_button_when_there_is_something_to_debug() {
        let plain = WidgetState {
            selected: Some("Dev server".to_owned()),
            rows: vec![row("Dev server", Origin::Permanent, false)],
            ..WidgetState::default()
        };
        let with_debug = WidgetState { debuggable: true, ..plain.clone() };
        assert_eq!(width(&with_debug) - width(&plain), 2.0 + BUTTON);
        // And both at once is both, which is what a running debug session looks like.
        let running = WidgetState { running: true, ..with_debug.clone() };
        assert_eq!(width(&running) - width(&plain), 2.0 * (2.0 + BUTTON));
    }

    #[test]
    fn the_widget_grows_by_one_button_while_something_is_running() {
        let idle = WidgetState {
            selected: Some("Dev server".to_owned()),
            rows: vec![row("Dev server", Origin::Permanent, false)],
            ..WidgetState::default()
        };
        let running = WidgetState { running: true, ..idle.clone() };
        assert_eq!(width(&running) - width(&idle), 2.0 + BUTTON);
    }

    #[test]
    fn the_button_is_the_same_size_as_the_text_tools_beside_it() {
        // Two nearly-equal sizes in one bar would read as a mistake.
        assert_eq!(BUTTON, crate::components::text_tools::BUTTON);
    }

    #[test]
    fn the_name_says_run_until_something_is_chosen() {
        let mut state = WidgetState {
            rows: vec![row("Dev server", Origin::Permanent, false)],
            ..WidgetState::default()
        };
        assert_eq!(state.label(), "Run");
        state.selected = Some("Dev server".to_owned());
        assert_eq!(state.label(), "Dev server");
    }
}
