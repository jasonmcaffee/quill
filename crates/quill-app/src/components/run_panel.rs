//! The run tile along the bottom of the window: one tab per run, the three buttons that act on the
//! one that is showing, and the grid the program is watched in.
//!
//! It is the terminal tile's **sibling**, not a second tile that resembles it: the same header
//! height, the same padding, the same splitter above it, and the grid itself is
//! `terminal_panel::grid`, shared rather than copied. A run *is* a terminal as far as the person
//! watching it is concerned — the same emulator, the same colours, the same selection, the same
//! clipboard rules — and keyboard into the program, because `node` asking a question deserves an
//! answer. `tasks/task-1683-run-configurations-tdd.md` §6.1 is the design.
//!
//! **The bottom of the window shows either the terminal tile or this one**, never both stacked: two
//! grids stacked take the editing area below the fold of anything. The activity bar's bottom group
//! holds a button each and pressing one puts the other away, which `QuillApp` settles, not this
//! file.
//!
//! ## What a run is
//!
//! A [`Run`] holds the configuration's **snapshot** rather than its name, so editing a
//! configuration mid-run changes what the next run does and never what the tab says about the one
//! that already happened. When the program stops the tab stays, holding everything it wrote, with
//! the exit code in the strip — `finished`, or `exit code 101` in the error colour. **The grid is
//! never written into by Quill**: IntelliJ prints its epilogue into the console, and that line
//! pretending to be program output is exactly the confusion a separate strip avoids.
//!
//! ## Stopping
//!
//! Soft, then hard, which is IntelliJ's rule. The first press is the interrupt byte down the pty —
//! the Ctrl+C a program can catch and tidy up after. A program still alive after [`GRACE`], or a
//! second press, is killed through [`quill_terminal::Session::kill`]. Closing a tab and closing the
//! window take the same path, so nothing ever orphans a child on purpose.

use std::time::{Duration, Instant};

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_terminal::session::{Session, SessionSettings, Size};

use crate::components::controls;
use crate::components::splitter;
use crate::components::terminal_panel;
use crate::services::run_configurations::Configuration;
use crate::services::text_renderer::TextRenderer;
use crate::theme::{color, icon};

/// How tall the strip holding the tabs is. The terminal tile's, because they are siblings.
pub const HEADER: f32 = terminal_panel::HEADER;

/// How long a program is given to answer the polite stop before the hard one follows.
///
/// Two seconds, which is IntelliJ's own grace and is short on purpose: the person pressing stop has
/// already decided, and a second press does not wait at all.
pub const GRACE: Duration = Duration::from_secs(2);

/// One run: what was started, and the program running it.
pub struct Run {
    /// A **snapshot** of the configuration as it was when this run started. See the file's own
    /// documentation for why it is not the name.
    pub configuration: Configuration,
    pub session: Session,
    /// When the polite stop was sent, so the hard one can follow after [`GRACE`].
    stopping: Option<Instant>,
    /// What the program ended with, once it has. `None` while it is still going; `Some(None)` for
    /// one that ended without choosing a code, which is a program Quill killed or, on Unix, one a
    /// signal took.
    ///
    /// **Recorded rather than re-read.** `Session::exit_code` is the source, and this is where the
    /// answer is kept the moment it arrives: a run that was killed has no code to be asked for
    /// afterwards, and a tab that had said `exit code 101` must go on saying it for as long as the
    /// tab is there. It is also what lets a screenshot test draw a finished tab — see
    /// [`RunPanel::end_detached`].
    ended: Option<Option<i32>>,
}

impl Run {
    /// What the tab is called, which is the configuration's name.
    pub fn name(&self) -> &str {
        &self.configuration.name
    }

    pub fn is_running(&self) -> bool {
        self.ended.is_none()
    }

    /// What the program ended with, or `None` while it is still going or if it chose no code.
    pub fn exit_code(&self) -> Option<i32> {
        self.ended.flatten()
    }

    /// What the strip says about this run's state, and in which colour.
    ///
    /// One function rather than the arithmetic being done at each of the three places that ask —
    /// the tab, the command line's `run status` and `run list` — so they cannot come to different
    /// answers about what `exit code 101` means.
    pub fn state(&self) -> State {
        match self.ended {
            None => State::Running,
            Some(Some(0)) => State::Finished,
            Some(Some(code)) => State::Failed(code),
            // A program whose code could not be read — one Quill killed, or one a signal took on
            // Unix — is honestly "stopped" rather than a number nobody measured.
            Some(None) => State::Stopped,
        }
    }
}

/// What a run's tab says about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Going. A green dot.
    Running,
    /// Ended with nothing to report.
    Finished,
    /// Ended with a code that is not zero, which is drawn in the error colour.
    Failed(i32),
    /// Killed, or ended in a way that carried no code.
    Stopped,
}

impl State {
    /// The word the strip, the command line and a test all use.
    pub fn label(&self) -> String {
        match self {
            State::Running => "running".to_owned(),
            State::Finished => "finished".to_owned(),
            State::Failed(code) => format!("exit code {code}"),
            State::Stopped => "stopped".to_owned(),
        }
    }

    pub fn is_running(&self) -> bool {
        *self == State::Running
    }

    /// The colour it is drawn in: git's added green while it is going, the error red for a code
    /// that is not zero, and the quiet colour for a run that simply ended. Both come from
    /// `theme::color`, which is the whole list of colours Quill draws with.
    pub fn tint(&self) -> egui::Color32 {
        match self {
            State::Running => color::GIT_ADDED,
            State::Failed(_) => color::CLOSE,
            State::Finished | State::Stopped => color::TEXT_DIM,
        }
    }
}

/// The run tile's own state: the runs, which one is showing, and whether the tile is up.
pub struct RunPanel {
    /// False when the tile is put away, which is what `View -> Run Tile` and the rail's button
    /// switch.
    pub visible: bool,
    /// True when the keyboard is talking to the run rather than to the document.
    pub focused: bool,
    runs: Vec<Run>,
    active: usize,
    /// True while the mouse is dragging out a selection.
    selecting: bool,
    /// The rectangle the grid last filled, for the tests.
    grid_area: Rect,
    /// The rectangle the tile has along the bottom of the window, whether it is showing or not.
    ///
    /// Recorded by the window every frame rather than by this file when it draws, because a run
    /// **started while the tile is put away** has to be given the right size too — and a session
    /// given the wrong size is one that gets resized a millisecond later, which is what
    /// `QuillApp::run_grid_size` explains and what loses a fast program's output.
    pub tile: Rect,
}

impl Default for RunPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl RunPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            focused: false,
            runs: Vec::new(),
            active: 0,
            selecting: false,
            grid_area: Rect::ZERO,
            tile: Rect::ZERO,
        }
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    pub fn count(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> Option<&Run> {
        self.runs.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Run> {
        self.runs.get_mut(self.active)
    }

    pub fn at(&self, index: usize) -> Option<&Run> {
        self.runs.get(index)
    }

    /// Show run `index`. A number past the end is ignored rather than clamped, which is
    /// `Tabs::show`'s rule: clamping would show a tab nobody asked for.
    pub fn show(&mut self, index: usize) {
        if index < self.runs.len() {
            self.active = index;
        }
    }

    /// Which run belongs to a configuration of this name, if one does.
    ///
    /// There is at most one, because running a configuration that is already running is a rerun
    /// rather than a second copy — §5.2, and IntelliJ's own default.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.runs.iter().position(|run| run.name() == name)
    }

    /// Where the grid was last drawn, which a test uses to work out where a cell is on screen.
    pub fn grid_area(&self) -> Rect {
        self.grid_area
    }

    /// The name of each tab, in order.
    ///
    /// No numbering, unlike the terminal's: two runs cannot share a name, because a second run of
    /// one configuration replaces the first.
    pub fn names(&self) -> Vec<String> {
        self.runs.iter().map(|run| run.name().to_owned()).collect()
    }

    /// Start a configuration, replacing the run it already has if it has one.
    ///
    /// **One instance per configuration**: pressing run on something that is already running stops
    /// it and starts it again, which is the honest reading of what a person means by pressing run
    /// on `Dev server` twice — the port is taken; they want the new code. A second simultaneous run
    /// of the same command is two configurations with two names, which also gives the two tabs two
    /// names.
    ///
    /// The reply is the index of the run, or the reason the program could not be started, which the
    /// caller puts in the status bar rather than this deciding what to say about it.
    pub fn start(
        &mut self,
        configuration: Configuration,
        root: &std::path::Path,
        size: Size,
        waker: quill_terminal::session::Waker,
    ) -> Result<usize, String> {
        let Some((program, args)) = configuration.program_and_arguments() else {
            return Err(format!("{} has no command to run.", configuration.name));
        };
        let settings = SessionSettings {
            shell: Some(program.clone()),
            args,
            working_directory: Some(configuration.working_directory(root)),
            // **What the person's shell would have given the command**, and the configuration's own
            // variables after it so one that names the same variable still wins. A Quill started from
            // the Finder has about a dozen variables from launchd and a `PATH` of four folders, so
            // `node` and `cargo` under a version manager could not be found at all — the fault
            // `run_configurations::found_on_path` already describes and could only report — and a
            // program that reads a token or a certificate bundle out of the environment got neither.
            // `services::login_shell` is that profile, read once.
            env: {
                let mut environment = crate::services::login_shell::for_a_child();
                environment.extend(configuration.environment());
                environment
            },
        };
        let session = Session::spawn(&settings, size, waker)
            .map_err(|problem| format!("Quill could not start {program}: {problem}"))?;
        // The one that was there is stopped and thrown away first, so its tab does not outlive the
        // rerun that replaced it.
        if let Some(at) = self.index_of(&configuration.name) {
            self.runs[at].session.kill();
            self.runs.remove(at);
        }
        self.runs.push(Run { configuration, session, stopping: None, ended: None });
        self.active = self.runs.len() - 1;
        Ok(self.active)
    }

    /// Add a run with no program behind it, fed bytes directly.
    ///
    /// What the screenshot tests use, exactly as `Tabs::open_detached` is what the terminal's use:
    /// when a real program answers is not something a test can know, so a picture of a run is taken
    /// of an emulator that was handed fixed bytes.
    pub fn start_detached(&mut self, configuration: Configuration, size: Size) -> usize {
        self.runs.push(Run {
            configuration,
            session: Session::detached(size),
            stopping: None,
            ended: None,
        });
        self.active = self.runs.len() - 1;
        self.active
    }

    /// Say that a detached run has ended, with the code it chose or with none at all.
    ///
    /// The other half of [`Self::start_detached`], and it exists for the same reason: a detached
    /// session has no child, so it can never end on its own, and a picture of a tab that says
    /// `exit code 101` cannot be taken of a real program without waiting for one. The state is
    /// recorded on the run rather than read back out of the session, so this sets exactly what a
    /// real ending would have set.
    pub fn end_detached(&mut self, index: usize, code: Option<i32>) {
        if let Some(run) = self.runs.get_mut(index) {
            run.ended = Some(code);
            run.stopping = None;
        }
    }

    /// Stop run `index`: politely the first time, and for good the second.
    ///
    /// Returns false when there is no such run, so a caller can say so.
    pub fn stop(&mut self, index: usize) -> bool {
        let Some(run) = self.runs.get_mut(index) else {
            return false;
        };
        if run.ended.is_some() {
            return true;
        }
        match run.stopping {
            // The second press does not wait: whoever pressed it has already decided.
            Some(_) => {
                run.session.kill();
                run.ended = Some(None);
                run.stopping = None;
            }
            None => {
                run.session.interrupt();
                run.stopping = Some(Instant::now());
            }
        }
        true
    }

    /// Kill run `index` outright, with no grace at all.
    ///
    /// What closing a tab, closing the window and a rerun all do. Nothing ever orphans a child on
    /// purpose.
    pub fn kill(&mut self, index: usize) {
        if let Some(run) = self.runs.get_mut(index) {
            run.stopping = None;
            if run.ended.is_none() {
                run.session.kill();
                run.ended = Some(None);
            }
        }
    }

    /// Throw away what run `index` has written, which is the tile's third button.
    pub fn clear(&mut self, index: usize) -> bool {
        match self.runs.get_mut(index) {
            Some(run) => {
                run.session.clear();
                true
            }
            None => false,
        }
    }

    /// Close run `index`, killing its program. The tab to its left is shown next.
    pub fn close(&mut self, index: usize) {
        if index >= self.runs.len() {
            return;
        }
        self.kill(index);
        self.runs.remove(index);
        if self.active >= self.runs.len() {
            self.active = self.runs.len().saturating_sub(1);
        }
    }

    /// Deal with what every run's program has said, and follow up a polite stop that was ignored.
    ///
    /// Called once a frame. **A run whose program has ended is kept**, unlike a terminal tab, which
    /// closes itself: the exit code is the point, and a tab that vanished at the moment it had
    /// something to say would be the one thing a run panel must not do.
    ///
    /// Returns true when a program was killed here, so the window can draw again and say so.
    pub fn settle(&mut self) -> bool {
        let mut killed = false;
        for run in self.runs.iter_mut() {
            run.session.pump();
            if run.ended.is_some() {
                continue;
            }
            // The moment the program ends, what it ended with is written down: a killed run has no
            // code to be asked for afterwards, and a tab has to go on saying what it said.
            if !run.session.is_running() {
                run.ended = Some(run.session.exit_code());
                run.stopping = None;
                continue;
            }
            if run.stopping.is_some_and(|since| since.elapsed() >= GRACE) {
                run.session.kill();
                run.ended = Some(None);
                run.stopping = None;
                killed = true;
            }
        }
        killed
    }

    /// How long until the earliest polite stop's grace runs out, or `None` when nothing is waiting.
    ///
    /// The window asks egui to draw again **then** rather than on every frame until then. An idle
    /// window draws nothing, so the hard kill has to be woken for; waking sixty times a second for
    /// two seconds in order to do one thing at the end of them would be a busy loop, and one a
    /// person would hear in the fan.
    pub fn stopping_in(&self) -> Option<Duration> {
        self.runs
            .iter()
            .filter_map(|run| run.stopping)
            .map(|since| GRACE.saturating_sub(since.elapsed()))
            .min()
    }

    /// True while something is waiting for the grace to run out.
    pub fn is_stopping(&self) -> bool {
        self.stopping_in().is_some()
    }

    /// Stop every program, which is what closing the window does.
    pub fn kill_everything(&mut self) {
        for index in 0..self.runs.len() {
            self.kill(index);
        }
    }
}

/// What the tile asks the window to do.
#[derive(Debug, Default)]
pub struct RunOutcome {
    /// The tile was put away.
    pub hide: bool,
    /// The tile was clicked, so it should have the keyboard.
    pub take_focus: bool,
    /// Text to put on the clipboard, from a copy or from a program asking.
    pub copy: Option<String>,
    /// The tile is being carried to another edge of the window, or its header was right clicked.
    ///
    /// The divider that resizes it is drawn by the window now rather than here: since `task-1697`
    /// the tile is not always along the bottom, so its inner edge is not always its top.
    pub grab: crate::components::dock::Grab,
    /// The rerun button was pressed, for the run that is showing.
    pub rerun: bool,
    /// The stop button was pressed, which the tile has already acted on. Reported so the window
    /// can say what happened in the status bar.
    pub stop: bool,
}

/// Draw the tile into `area` and take its input.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut RunPanel,
    renderer: &TextRenderer,
    font_size: f32,
    opacity: f32,
) -> RunOutcome {
    let mut outcome = RunOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::TOOLBAR, opacity));

    let header =
        Rect::from_min_size(Pos2::new(area.left(), area.top() + 1.0), Vec2::new(area.width(), HEADER));
    show_header(ui, header, panel, &mut outcome);
    splitter::line(
        &ui.painter_at(area),
        Pos2::new(area.left(), header.bottom()),
        Pos2::new(area.right(), header.bottom()),
    );

    let grid = Rect::from_min_max(Pos2::new(area.left(), header.bottom() + 1.0), area.max);
    panel.grid_area = grid;
    let focused = panel.focused;
    let mut selecting = panel.selecting;
    let grid_outcome = terminal_panel::grid(
        ui,
        grid,
        panel.runs.get_mut(panel.active).map(|run| &mut run.session),
        &mut selecting,
        focused,
        "run-grid",
        "Nothing is running. Press the play button in the title bar to start something.",
        renderer,
        font_size,
        opacity,
    );
    panel.selecting = selecting;
    outcome.take_focus |= grid_outcome.take_focus;
    if let Some(text) = grid_outcome.copy {
        outcome.copy = Some(text);
    }
    outcome
}

/// The strip along the top: the word `Run`, the tabs, and the buttons for the tab that is showing.
fn show_header(ui: &mut egui::Ui, area: Rect, panel: &mut RunPanel, outcome: &mut RunOutcome) {
    // The handle first, over the whole strip, so the tabs and the buttons added after it take the
    // points they cover and this is left with the heading and the empty space beside it. See
    // `components::dock` for why it has to be this way round.
    outcome.grab = crate::components::dock::handle(ui, area, crate::app::dock::Panel::Run);
    let painter = ui.painter_at(area);
    let heading =
        painter.layout_no_wrap("Run".to_owned(), egui::FontId::proportional(12.0), color::TEXT_DIM);
    painter.galley(
        Pos2::new(area.left() + 16.0, area.center().y - heading.size().y / 2.0),
        heading.clone(),
        color::TEXT_DIM,
    );

    // The buttons first, so the strip of tabs knows where it has to stop and a long list of names
    // never runs underneath one.
    let buttons_left = show_buttons(ui, area, panel, outcome);
    tab_strip(ui, area, area.left() + 16.0 + heading.size().x + 18.0, buttons_left, panel, outcome);
}

/// The four buttons at the right hand end. Returns the x they start at.
///
/// `rerun` is reported, because starting something needs the project folder, the grid size and the
/// waker, none of which a component has. `stop` and `clear` are done here through the panel's own
/// methods — the same ones the menu and the command line call — because both touch nothing but the
/// session the tile already has.
///
/// `stop` is **dimmed** once the program has gone rather than absent, because it could apply again
/// in a moment, which is exactly what dimming means and is the distinction `A control is absent
/// when it cannot apply` draws.
fn show_buttons(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut RunPanel,
    outcome: &mut RunOutcome,
) -> f32 {
    let mut right = area.right() - 22.0;
    let hide = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if controls::icon_button(ui, hide, "Hide the run tile", icon::collapse) {
        outcome.hide = true;
    }
    right -= 26.0;

    let index = panel.active_index();
    let has_run = panel.active().is_some();
    let running = panel.active().is_some_and(Run::is_running);

    let clear = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if dimmable_button(ui, clear, "Clear the run output", icon::clear, has_run) {
        panel.clear(index);
    }
    right -= 26.0;

    let stop = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if dimmable_button(ui, stop, "Stop the run", icon::stop, running) {
        panel.stop(index);
        outcome.stop = true;
    }
    right -= 26.0;

    let rerun = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if dimmable_button(ui, rerun, "Rerun", icon::rerun, has_run) {
        outcome.rerun = true;
    }
    right - 16.0
}

/// An icon button that is dimmed and unclickable when what it does cannot be done just now.
///
/// `controls::icon_button` has no such state, because until now every icon button in Quill could
/// always be pressed. Dimming rather than removing is what `A control is absent when it cannot
/// apply` asks for here: stop could apply again the moment the run is started again, and a button
/// that came and went under the pointer would be worse than one that waits.
fn dimmable_button(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    draw: fn(&egui::Painter, Pos2, egui::Color32),
    enabled: bool,
) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(area, ui.id().with(("run-button", name)), sense).on_hover_text(name);
    if response.hovered() && enabled {
        ui.painter().rect_filled(area, CornerRadius::same(4), color::CONTROL);
    }
    let tint = if enabled { color::TEXT_DIM } else { color::TEXT_FAINT.gamma_multiply(0.6) };
    draw(ui.painter(), area.center(), tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    response.clicked()
}

/// The tabs themselves, left to right from `pen`, stopping before `limit`.
fn tab_strip(
    ui: &mut egui::Ui,
    area: Rect,
    pen: f32,
    limit: f32,
    panel: &mut RunPanel,
    outcome: &mut RunOutcome,
) {
    let active = panel.active;
    let mut show: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut pen = pen;
    for index in 0..panel.runs.len() {
        let name = panel.runs[index].name().to_owned();
        let state = panel.runs[index].state();
        let rect = draw_tab(ui, area, pen, &name, state, index == active, index, &mut show, &mut close);
        pen = rect.right() + 6.0;
        if pen > limit {
            // A strip longer than the room there is stops rather than running under the buttons.
            // Six or seven runs is already more than anybody watches at once, and the tab that is
            // showing is always drawn because the loop starts from the left.
            break;
        }
    }
    if let Some(index) = close {
        panel.close(index);
        if panel.is_empty() {
            outcome.hide = true;
        }
    } else if let Some(index) = show {
        panel.show(index);
        outcome.take_focus = true;
    }
}

/// One tab: the pill, a state marker, the name, and the close cross. Returns the rectangle it
/// filled.
#[allow(clippy::too_many_arguments)]
fn draw_tab(
    ui: &mut egui::Ui,
    area: Rect,
    pen: f32,
    name: &str,
    state: State,
    active: bool,
    index: usize,
    show: &mut Option<usize>,
    close: &mut Option<usize>,
) -> Rect {
    let painter = ui.painter_at(area);
    let label = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        if active { color::TEXT_STRONG } else { color::TEXT_CONTROL },
    );
    // The state is written beside the name rather than in the grid: IntelliJ prints its epilogue
    // into the console, and a line pretending to be program output is the confusion this avoids.
    let note = match state {
        State::Running => None,
        other => Some(painter.layout_no_wrap(
            other.label(),
            egui::FontId::proportional(11.0),
            other.tint(),
        )),
    };
    let note_width = note.as_ref().map(|galley| galley.size().x + 8.0).unwrap_or(0.0);
    let tab = Rect::from_min_size(
        Pos2::new(pen, area.center().y - 11.0),
        Vec2::new(label.size().x + note_width + 48.0, 22.0),
    );
    let response = ui
        .interact(tab, ui.id().with(("run-tab", index)), Sense::click())
        .on_hover_text(format!("Run: {name} \u{00B7} {}", state.label()));
    if active {
        painter.rect(
            tab,
            CornerRadius::same(4),
            color::SELECTED_ROW,
            Stroke::new(1.0, color::ACCENT.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(tab, CornerRadius::same(4), color::CONTROL);
    }
    let mut text_left = tab.left() + 10.0;
    if state.is_running() {
        icon::state_dot(&painter, Pos2::new(text_left + 2.0, tab.center().y), state.tint());
        text_left += 12.0;
    }
    painter.galley(
        Pos2::new(text_left, tab.center().y - label.size().y / 2.0),
        label.clone(),
        color::TEXT_CONTROL,
    );
    if let Some(note) = note {
        painter.galley(
            Pos2::new(
                text_left + label.size().x + 8.0,
                tab.center().y - note.size().y / 2.0,
            ),
            note.clone(),
            state.tint(),
        );
    }
    let shut =
        Rect::from_center_size(Pos2::new(tab.right() - 12.0, tab.center().y), Vec2::splat(16.0));
    let shut_response = ui
        .interact(shut, ui.id().with(("run-close", index)), Sense::click())
        .on_hover_text(format!("Close {name}"));
    icon::cross(&painter, shut.center(), color::TEXT_DIM);
    shut_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {name}"))
    });
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, format!("Run: {name}"))
    });
    if shut_response.clicked() || response.middle_clicked() {
        *close = Some(index);
    } else if response.clicked() {
        *show = Some(index);
    }
    tab
}

/// Space for the tile's own furniture, used when the window works out how tall to open it.
pub const FURNITURE: f32 = terminal_panel::FURNITURE;

/// How tall a tile with `rows` rows of text needs to be, which is the terminal's arithmetic because
/// the two tiles are the same shape.
pub fn height_for(rows: usize, cell_height: f32) -> f32 {
    terminal_panel::height_for(rows, cell_height)
}

/// The size of the grid a tile of this size holds.
pub fn grid_size(tile: Vec2, cell: crate::services::text_renderer::CellMetrics) -> Size {
    terminal_panel::grid_size(tile, cell)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detached(panel: &mut RunPanel, name: &str) -> usize {
        panel.start_detached(Configuration::new(name, "node server.js"), Size::new(8, 40))
    }

    #[test]
    fn a_new_tile_has_nothing_in_it_and_is_put_away() {
        let panel = RunPanel::new();
        assert!(!panel.visible);
        assert!(panel.is_empty());
        assert!(panel.active().is_none());
    }

    #[test]
    fn starting_a_run_shows_it_and_the_tab_is_named_after_the_configuration() {
        let mut panel = RunPanel::new();
        detached(&mut panel, "Dev server");
        detached(&mut panel, "cargo run");
        assert_eq!(panel.names(), vec!["Dev server", "cargo run"]);
        assert_eq!(panel.active_index(), 1, "the new one is the one showing");
        panel.show(0);
        assert_eq!(panel.active_index(), 0);
        panel.show(9);
        assert_eq!(panel.active_index(), 0, "a run that is not there is not shown");
    }

    #[test]
    fn a_run_is_found_by_the_name_of_the_configuration_that_started_it() {
        let mut panel = RunPanel::new();
        detached(&mut panel, "Dev server");
        assert_eq!(panel.index_of("Dev server"), Some(0));
        assert_eq!(panel.index_of("cargo run"), None);
    }

    #[test]
    fn closing_a_run_leaves_the_one_to_its_left_showing() {
        let mut panel = RunPanel::new();
        for name in ["one", "two", "three"] {
            detached(&mut panel, name);
        }
        assert_eq!(panel.active_index(), 2);
        panel.close(2);
        assert_eq!(panel.count(), 2);
        assert_eq!(panel.active_index(), 1);
        panel.close(0);
        assert_eq!(panel.names(), vec!["two"]);
        panel.close(0);
        assert!(panel.is_empty());
        panel.close(0);
        assert!(panel.is_empty(), "closing a run that is not there does nothing");
    }

    #[test]
    fn a_run_that_has_ended_keeps_its_tab_and_says_what_it_ended_with() {
        // Unlike a terminal tab, which closes itself when its shell goes: the exit code is the
        // point, and a tab that vanished at the moment it had something to say would be the one
        // thing a run panel must not do. A detached session never ends on its own, so the states
        // are checked through `State` rather than by running something here — the real thing is
        // exercised by `quill-terminal`'s own tests and by the walkthrough.
        assert_eq!(State::Running.label(), "running");
        assert_eq!(State::Finished.label(), "finished");
        assert_eq!(State::Failed(101).label(), "exit code 101");
        assert_eq!(State::Stopped.label(), "stopped");
        assert_eq!(State::Failed(101).tint(), color::CLOSE, "a bad code is in the error colour");
        assert_eq!(State::Running.tint(), color::GIT_ADDED);
        assert!(State::Running.is_running());
        assert!(!State::Finished.is_running());
    }

    #[test]
    fn a_detached_run_is_running_until_it_is_killed_and_then_says_stopped() {
        let mut panel = RunPanel::new();
        detached(&mut panel, "Dev server");
        assert_eq!(panel.active().expect("a run").state(), State::Running);
        panel.kill(0);
        assert_eq!(panel.active().expect("a run").state(), State::Stopped);
        // And what it wrote is still there, which is what makes the evidence outlive the process.
        panel.runs[0].session.feed(b"the output");
        assert!(panel.runs[0].session.snapshot().contains("the output"));
    }

    #[test]
    fn a_run_that_ended_goes_on_saying_what_it_ended_with() {
        // The state is recorded the moment it arrives rather than re-read, so a tab that says
        // `exit code 101` goes on saying it and a killed run does not have to invent a code.
        let mut panel = RunPanel::new();
        detached(&mut panel, "cargo test");
        panel.end_detached(0, Some(101));
        assert_eq!(panel.at(0).expect("a run").state(), State::Failed(101));
        assert_eq!(panel.at(0).expect("a run").exit_code(), Some(101));
        assert!(!panel.at(0).expect("a run").is_running());
        // Settling it again leaves it exactly as it was.
        panel.settle();
        assert_eq!(panel.at(0).expect("a run").state(), State::Failed(101));
        panel.end_detached(1, Some(0));
        assert_eq!(panel.count(), 1, "ending a run that is not there does nothing");
    }

    #[test]
    fn stopping_is_polite_first_and_hard_second() {
        let mut panel = RunPanel::new();
        detached(&mut panel, "Dev server");
        assert!(panel.stop(0), "the first press is the interrupt, and the program is given a moment");
        assert!(panel.at(0).expect("a run").is_running());
        assert!(panel.is_stopping(), "which is what keeps the window drawing until the grace is up");
        assert!(panel.stop(0), "the second press does not wait");
        assert!(!panel.at(0).expect("a run").is_running());
        assert_eq!(panel.at(0).expect("a run").state(), State::Stopped);
        assert!(!panel.is_stopping());
        assert!(panel.stop(0), "stopping something that has stopped is not a failure");
        assert!(!panel.stop(9), "and stopping a run that is not there is");
    }

    #[test]
    fn clearing_throws_away_what_the_run_wrote_and_leaves_the_tab() {
        let mut panel = RunPanel::new();
        detached(&mut panel, "Dev server");
        panel.runs[0].session.feed(b"Listening on 3000");
        assert!(panel.runs[0].session.snapshot().contains("Listening"));
        assert!(panel.clear(0));
        assert_eq!(panel.runs[0].session.snapshot().text(), "");
        assert_eq!(panel.count(), 1, "the tab stays");
        assert!(!panel.clear(9));
    }

    #[test]
    fn killing_everything_stops_every_run() {
        let mut panel = RunPanel::new();
        for name in ["one", "two"] {
            detached(&mut panel, name);
        }
        panel.kill_everything();
        assert!(panel.runs().iter().all(|run| !run.is_running()));
    }

    #[test]
    fn a_run_holds_the_configuration_as_it_was_when_it_started() {
        // Editing a configuration mid-run changes what the next run does and never what the tab
        // says about the one that already happened.
        let mut panel = RunPanel::new();
        panel.start_detached(
            Configuration::new("Dev server", "node server.js --port 3000"),
            Size::new(8, 40),
        );
        assert_eq!(panel.at(0).expect("a run").configuration.command, "node server.js --port 3000");
    }

    #[test]
    fn the_tile_is_the_same_shape_as_the_terminals() {
        // Siblings, drawn from the same measurements. Two tiles that almost agreed would be two
        // grids that did not line up when the bottom of the window was switched between them.
        assert_eq!(HEADER, terminal_panel::HEADER);
        assert_eq!(FURNITURE, terminal_panel::FURNITURE);
        let cell = crate::services::text_renderer::CellMetrics { width: 8.0, height: 17.0, ascent: 13.0 };
        assert_eq!(height_for(12, cell.height), terminal_panel::height_for(12, cell.height));
        assert_eq!(grid_size(Vec2::new(400.0, 300.0), cell), terminal_panel::grid_size(Vec2::new(400.0, 300.0), cell));
    }
}
