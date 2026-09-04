//! The debug tile along the bottom of the window: the stepping buttons, the call stack, and the
//! variables and watches beside it.
//!
//! It is the run tile's **sibling** as the run tile is the terminal's: the same header height, the
//! same padding, the same splitter above it, and the same `settings::Panes` treatment of its height.
//! `tasks/task-1687-debugging-tdd.md` §9 is the design.
//!
//! **The bottom of the window holds one of three tiles now** — terminal, run, debug — never two
//! stacked, because two grids stacked take the editing area below the fold of anything. That is
//! settled by `QuillApp::show_the_debug_tile` and its two siblings, not here.
//!
//! **The console is the run tile, not a pane inside this one.** The debuggee's output goes to a real
//! terminal through `runInTerminal` (§7.2), and stacking a second grid inside this tile was already
//! rejected once for the run tile, for the reason above. So the header ends with a `Console` button
//! that swaps to the run tile and back, one press, both directions.
//!
//! Like every component in Quill this one takes a rectangle, draws itself and reports what happened.
//! It changes no state, it speaks no protocol, and it holds no opinion about what a breakpoint is:
//! `app::debug::DebugState` decides all of that.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::debug::{DebugState, Row, Watch};
use crate::components::controls;
use crate::components::splitter;
use crate::components::terminal_panel;
use crate::theme::{color, icon, size};

/// How tall the strip holding the buttons is. The run tile's, because they are siblings.
pub const HEADER: f32 = terminal_panel::HEADER;
/// How tall one row of the frames list and the variables tree is. The style guide's list row.
const ROW: f32 = size::ROW;
/// How far each level of the tree is indented.
pub const INDENT: f32 = 14.0;
/// How wide the frames pane is by default, as a proportion of the tile.
const FRAMES_SHARE: f32 = 0.34;
/// The narrowest either pane is allowed to be, so a divider dragged to the edge still leaves
/// something to read.
const PANE_MIN: f32 = 140.0;
/// How tall the strip of watches is when there are any.
const WATCH_HEADER: f32 = 24.0;
/// A value longer than this is elided in the tree, because a row that is a paragraph long pushes
/// every other row off the screen and cannot be read anyway.
pub const VALUE_LIMIT: usize = 220;

/// What the tile draws when there is no session, which is when it has the most to say.
///
/// An empty box is what the feature looked like to somebody who had not read the documentation —
/// `task-1692`'s first sentence — so the tile answers the three questions there are: is something
/// happening, is there a debugger on this machine, and what do I press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Idle {
    /// Nothing is happening and everything is in place, so it says what starts one.
    Ready(String),
    /// A locator's build is running, and this is what it is and how long it has been going.
    Building { what: String, seconds: u64 },
    /// There is no adapter on this machine, so it says what is missing and offers the command.
    Missing(Missing),
}

impl Default for Idle {
    fn default() -> Self {
        Idle::Ready(String::new())
    }
}

/// A debugger this machine has not got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Missing {
    /// The adapter's own name, which is what the Install button asks for.
    pub adapter: String,
    /// What was looked for and where it comes from, as the registry entry says it.
    pub sentence: String,
    /// The command that would install it, empty when Quill has nothing to offer.
    pub install: String,
}

/// What the person did in one row of a variables tree.
///
/// Its own type rather than the whole of [`DebugOutcome`], because [`show_row`] is drawn in two
/// places now: the tile's tree, and the value tooltip `task-1696` hangs off a name in the source.
/// One row means one thing in both, which is the rule the disclosure triangle already follows.
#[derive(Debug, Default, PartialEq)]
pub struct RowOutcome {
    /// A row was opened or closed.
    pub toggle_row: Option<String>,
    /// A row was given a new value.
    pub set_value: Option<(String, String)>,
}

/// What the person did in the tile.
#[derive(Debug, Default, PartialEq)]
pub struct DebugOutcome {
    /// The Install button was pressed, and this is the adapter it is about.
    pub install: Option<String>,
    /// The Copy button was pressed, and this is the command to put on the clipboard.
    pub copy: Option<String>,
    /// The tile was put away.
    pub hide: bool,
    /// The `Console` button was pressed, so the run tile should come up in this one's place.
    pub console: bool,
    /// A stepping button was pressed.
    pub step: Option<quill_dap::Step>,
    /// The stop button was pressed.
    pub stop: bool,
    /// A frame was clicked, so its variables and the execution point should move to it.
    pub show_frame: Option<i64>,
    /// A row of the tree was opened or closed.
    pub toggle_row: Option<String>,
    /// A row was given a new value, which is `setVariable`.
    pub set_value: Option<(String, String)>,
    /// An expression was typed into the watch field.
    pub add_watch: Option<String>,
    /// A watch was taken off the list.
    pub remove_watch: Option<String>,
    /// An exception filter was ticked or unticked, and this is the whole set that is now on.
    pub filters: Option<Vec<String>>,
    /// The tile is being carried to another edge of the window, or was right clicked on its header.
    ///
    /// The divider that resizes it is drawn by the window now rather than here, because since
    /// `task-1697` the tile is not always along the bottom and its inner edge is not always its top.
    pub grab: crate::components::dock::Grab,
}

/// The tile's own state: what is being typed into it, and where its divider is.
///
/// A component holds no state, so what has to survive between frames lives here and is owned by the
/// window — the same arrangement `CommitPanel` and `RunDialog` already have.
#[derive(Debug, Clone)]
pub struct DebugPanel {
    /// False when the tile is put away, which is what `View -> Debug Tile` and the rail's button
    /// switch.
    pub visible: bool,
    /// Where the divider between the frames and the variables is, as a proportion of the tile.
    pub divider: f32,
    /// What is being typed into the watch field.
    pub watch: String,
    /// The row being edited, and what has been typed into it. `Set Value` turns a cell into a field,
    /// which is `controls::field_text_rect`'s five-fields lesson applied once more.
    pub editing: Option<(String, String)>,
    /// The rectangle the tile has along the bottom of the window, whether it is showing or not.
    /// Recorded by the window every frame, which is `RunPanel::tile`'s arrangement.
    pub tile: Rect,
}

impl Default for DebugPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            divider: FRAMES_SHARE,
            watch: String::new(),
            editing: None,
            tile: Rect::ZERO,
        }
    }

    /// Stop editing a cell, which is what resuming and ending a session both do: the row the field
    /// was over may not exist a moment from now.
    pub fn stop_editing(&mut self) {
        self.editing = None;
    }
}

/// Draw the tile into `area` and take its input.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut DebugPanel,
    debug: Option<&DebugState>,
    idle: &Idle,
    opacity: f32,
) -> DebugOutcome {
    let mut outcome = DebugOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::toolbar(), opacity));

    let header = Rect::from_min_size(
        Pos2::new(area.left(), area.top() + 1.0),
        Vec2::new(area.width(), HEADER),
    );
    show_header(ui, header, debug, &mut outcome);
    splitter::line(
        &ui.painter_at(area),
        Pos2::new(area.left(), header.bottom()),
        Pos2::new(area.right(), header.bottom()),
    );

    let body = Rect::from_min_max(Pos2::new(area.left(), header.bottom() + 1.0), area.max);
    let Some(debug) = debug else {
        // A tile with no session says what would start one, which is what the run tile's empty grid
        // does rather than showing an empty list nobody can act on. Since `task-1692` it also says
        // what is *stopping* one, and offers the command that would fix it.
        show_idle(ui, body, idle, &mut outcome);
        return outcome;
    };

    let split = (body.left() + body.width() * panel.divider.clamp(0.15, 0.85))
        .clamp(body.left() + PANE_MIN, (body.right() - PANE_MIN).max(body.left() + PANE_MIN));
    let frames_rect = Rect::from_min_max(body.min, Pos2::new(split, body.bottom()));
    let values_rect = Rect::from_min_max(Pos2::new(split + 1.0, body.top()), body.max);

    show_frames(ui, frames_rect, debug, &mut outcome);
    show_values(ui, values_rect, panel, debug, &mut outcome);

    // The divider, added after both panes for the reason `components::splitter` records: a pane
    // takes drags over the whole of its rectangle, so a divider added earlier is never seen.
    let line = Rect::from_min_size(Pos2::new(split, body.top()), Vec2::new(1.0, body.height()));
    let drag = splitter::show(ui, line, "debug-panes", splitter::Axis::Upright);
    if drag.delta != 0.0 && body.width() > 1.0 {
        panel.divider = (panel.divider + drag.delta / body.width()).clamp(0.15, 0.85);
    }
    if drag.reset {
        panel.divider = FRAMES_SHARE;
    }
    outcome
}

/// The strip along the top: the word `Debug`, the stepping buttons, where the session is, and the
/// two buttons at the right hand end.
fn show_header(
    ui: &mut egui::Ui,
    area: Rect,
    debug: Option<&DebugState>,
    outcome: &mut DebugOutcome,
) {
    // The handle first, over the whole strip, so the tabs and the buttons added after it take the
    // points they cover and this is left with the heading and the empty space beside it. See
    // `components::dock` for why it has to be this way round.
    outcome.grab = crate::components::dock::handle(ui, area, crate::app::dock::Panel::Debug);
    let painter = ui.painter_at(area);
    let heading =
        painter.layout_no_wrap("Debug".to_owned(), egui::FontId::proportional(12.0), color::text_dim());
    painter.galley(
        Pos2::new(area.left() + 16.0, area.center().y - heading.size().y / 2.0),
        heading.clone(),
        color::text_dim(),
    );

    let mut right = area.right() - 22.0;
    let hide = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if controls::icon_button(ui, hide, "Hide the debug tile", icon::collapse) {
        outcome.hide = true;
    }
    right -= 26.0;

    let console = Rect::from_center_size(Pos2::new(right, area.center().y), Vec2::splat(22.0));
    if controls::icon_button(ui, console, "Console", icon::terminal) {
        outcome.console = true;
    }
    right -= 30.0;

    // The five that act on the program, in the reference editor's own order. **Dimmed rather than absent** while
    // they cannot apply this instant, which is exactly what dimming means: a program that is running
    // will be stopped in a moment and these will all work again.
    let paused = debug.is_some_and(DebugState::is_paused);
    let alive = debug.is_some_and(DebugState::is_alive);
    let mut pen = area.left() + 16.0 + heading.size().x + 18.0;
    let mut button = |ui: &mut egui::Ui, name: &str, enabled: bool, draw: &dyn Fn(&egui::Painter, Pos2)| {
        let rect = Rect::from_center_size(Pos2::new(pen + 11.0, area.center().y), Vec2::splat(22.0));
        pen += 26.0;
        let pressed = dimmable(ui, rect, name, enabled, draw);
        pressed
    };
    if button(ui, "Resume", paused, &|painter, at| {
        icon::resume(painter, at, tint(paused))
    }) {
        outcome.step = Some(quill_dap::Step::Resume);
    }
    if button(ui, "Step Over", paused, &|painter, at| {
        icon::step(painter, at, icon::StepIcon::Over, tint(paused))
    }) {
        outcome.step = Some(quill_dap::Step::Over);
    }
    if button(ui, "Step Into", paused, &|painter, at| {
        icon::step(painter, at, icon::StepIcon::Into, tint(paused))
    }) {
        outcome.step = Some(quill_dap::Step::Into);
    }
    if button(ui, "Step Out", paused, &|painter, at| {
        icon::step(painter, at, icon::StepIcon::Out, tint(paused))
    }) {
        outcome.step = Some(quill_dap::Step::Out);
    }
    if button(ui, "Stop Debugging", alive, &|painter, at| {
        icon::stop(painter, at, tint(alive))
    }) {
        outcome.stop = true;
    }

    // The adapter's own exception filters, when it offered any. **Quill holds no list of its own**,
    // so an adapter that offers none gets no control at all.
    if let Some(debug) = debug {
        let offered = &debug.capabilities().exception_filters;
        if !offered.is_empty() {
            let rect =
                Rect::from_center_size(Pos2::new(pen + 11.0, area.center().y), Vec2::splat(22.0));
            pen += 30.0;
            let mut chosen: Vec<String> = debug.filters.clone();
            let changed = controls::flyout(ui, rect, "Exception Breakpoints", icon::bug, 240.0, |ui| {
                let mut changed = false;
                for filter in offered {
                    let mut on = chosen.contains(&filter.filter);
                    if ui.checkbox(&mut on, &filter.label).changed() {
                        changed = true;
                        match on {
                            true => chosen.push(filter.filter.clone()),
                            false => chosen.retain(|known| *known != filter.filter),
                        }
                    }
                }
                changed
            });
            if changed == Some(true) {
                outcome.filters = Some(chosen);
            }
        }
    }

    // Where the session is, in words, at the right hand end of what is left. The tint scheme
    // `run_panel::State` already uses: the accent while it is stopped, because that is when there is
    // something to do; the quiet colour otherwise.
    if let Some(debug) = debug {
        let said = debug.where_it_is();
        let tint = match debug.is_paused() {
            true => color::text_control(),
            false => color::text_dim(),
        };
        let label =
            ui.painter().layout_no_wrap(said, egui::FontId::proportional(11.5), tint);
        let x = (right - 16.0 - label.size().x).max(pen + 8.0);
        ui.painter().galley(
            Pos2::new(x, area.center().y - label.size().y / 2.0),
            label,
            tint,
        );
    }
}

/// The left pane: the frames of the stopped thread, with the thread's name above them when the
/// adapter reported more than one.
fn show_frames(ui: &mut egui::Ui, area: Rect, debug: &DebugState, outcome: &mut DebugOutcome) {
    let painter = ui.painter_at(area);
    let mut top = area.top() + 4.0;
    if debug.threads.len() > 1 {
        let name = debug
            .threads
            .iter()
            .find(|thread| Some(thread.id) == debug.frames.first().map(|_| thread.id))
            .map(|thread| thread.name.clone())
            .unwrap_or_else(|| format!("{} threads", debug.threads.len()));
        let label = painter.layout_no_wrap(name, egui::FontId::proportional(11.0), color::text_dim());
        painter.galley(Pos2::new(area.left() + 16.0, top + 3.0), label, color::text_dim());
        top += 22.0;
    }
    if debug.frames.is_empty() {
        empty(ui, Rect::from_min_max(Pos2::new(area.left(), top), area.max), "No stack while the program is running.");
        return;
    }
    let list = Rect::from_min_max(Pos2::new(area.left(), top), area.max);
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(list));
    scroll.set_clip_rect(ui.painter().clip_rect().intersect(list));
    egui::ScrollArea::vertical().id_salt("debug-frames").show(&mut scroll, |ui| {
        for frame in &debug.frames {
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(list.width(), ROW),
                Sense::click(),
            );
            let selected = debug.frame == Some(frame.id);
            if selected {
                ui.painter().rect_filled(
                    rect.shrink2(Vec2::new(6.0, 2.0)),
                    CornerRadius::same(size::CONTROL_CORNER),
                    color::selected_row(),
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    rect.shrink2(Vec2::new(6.0, 2.0)),
                    CornerRadius::same(size::CONTROL_CORNER),
                    color::control(),
                );
            }
            // A frame the adapter marked `subtle` — library internals — is drawn in the quiet
            // colour and **listed rather than hidden**, which is the comments-and-strings rule from
            // the references list.
            let tint = match (selected, frame.subtle) {
                (true, _) => color::text_strong(),
                (false, true) => color::text_faint(),
                (false, false) => color::text(),
            };
            let name = ui.painter().layout_no_wrap(
                frame.name.clone(),
                egui::FontId::proportional(12.0),
                tint,
            );
            let width = name.size().x;
            ui.painter().galley(
                Pos2::new(rect.left() + 16.0, rect.center().y - name.size().y / 2.0),
                name,
                tint,
            );
            if let Some(path) = &frame.path {
                let where_it_is = format!("{}:{}", file_name(path), frame.line);
                let label = ui.painter().layout_no_wrap(
                    where_it_is,
                    egui::FontId::proportional(10.5),
                    color::text_faint(),
                );
                ui.painter().galley(
                    Pos2::new(
                        rect.left() + 16.0 + width + 8.0,
                        rect.center().y - label.size().y / 2.0,
                    ),
                    label,
                    color::text_faint(),
                );
            }
            let accessible = format!("Frame: {}", frame.name);
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &accessible)
            });
            if response.clicked() {
                outcome.show_frame = Some(frame.id);
            }
        }
    });
}

/// The right pane: the watches above, the variables tree below.
fn show_values(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut DebugPanel,
    debug: &DebugState,
    outcome: &mut DebugOutcome,
) {
    let watch_height = WATCH_HEADER + debug.watches.len() as f32 * ROW + 6.0;
    let watches = Rect::from_min_size(
        area.min,
        Vec2::new(area.width(), watch_height.min(area.height() * 0.5)),
    );
    show_watches(ui, watches, panel, debug, outcome);
    splitter::line(
        &ui.painter_at(area),
        Pos2::new(area.left(), watches.bottom()),
        Pos2::new(area.right(), watches.bottom()),
    );
    let tree = Rect::from_min_max(Pos2::new(area.left(), watches.bottom() + 1.0), area.max);
    show_tree(ui, tree, panel, debug, outcome);
}

/// The watch list, with the field that adds one above it.
fn show_watches(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut DebugPanel,
    debug: &DebugState,
    outcome: &mut DebugOutcome,
) {
    let field = Rect::from_min_size(
        Pos2::new(area.left() + 12.0, area.top() + 3.0),
        Vec2::new((area.width() - 24.0).max(60.0), WATCH_HEADER - 4.0),
    );
    let response = controls::search_field(ui, field, "Watch", "Watch an expression", &mut panel.watch);
    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
        let expression = panel.watch.trim().to_owned();
        if !expression.is_empty() {
            outcome.add_watch = Some(expression);
            panel.watch.clear();
        }
    }
    let mut top = area.top() + WATCH_HEADER + 2.0;
    for watch in &debug.watches {
        if top + ROW > area.bottom() {
            break;
        }
        let row = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), ROW));
        show_watch(ui, row, watch, outcome);
        top += ROW;
    }
}

/// One watched expression: the expression, its answer, and the cross that takes it off the list.
fn show_watch(ui: &mut egui::Ui, row: Rect, watch: &Watch, outcome: &mut DebugOutcome) {
    let name = ui.painter().layout_no_wrap(
        watch.expression.clone(),
        egui::FontId::monospace(11.5),
        color::text(),
    );
    let width = name.size().x;
    ui.painter().galley(
        Pos2::new(row.left() + 16.0, row.center().y - name.size().y / 2.0),
        name,
        color::text(),
    );
    // The debugger's own answer, in its own words. A refusal is shown as it was written, because a
    // debugger explains a bad expression better than Quill could.
    let (said, tint) = match &watch.result {
        Some(Ok(value)) => (elide(&value.value), color::text_control()),
        Some(Err(problem)) => (elide(problem), color::close()),
        None => ("\u{2014}".to_owned(), color::text_faint()),
    };
    let value = ui.painter().layout_no_wrap(said, egui::FontId::monospace(11.5), tint);
    ui.painter().galley(
        Pos2::new(row.left() + 16.0 + width + 12.0, row.center().y - value.size().y / 2.0),
        value,
        tint,
    );
    let cross = Rect::from_center_size(
        Pos2::new(row.right() - 16.0, row.center().y),
        Vec2::splat(18.0),
    );
    if controls::icon_button(ui, cross, &format!("Remove watch: {}", watch.expression), icon::cross) {
        outcome.remove_watch = Some(watch.expression.clone());
    }
}

/// The variables tree.
fn show_tree(
    ui: &mut egui::Ui,
    area: Rect,
    panel: &mut DebugPanel,
    debug: &DebugState,
    outcome: &mut DebugOutcome,
) {
    if debug.rows.is_empty() {
        let message = match debug.is_paused() {
            true => "No variables in this frame.",
            false => "Variables are read while the program is stopped.",
        };
        empty(ui, area, message);
        return;
    }
    let can_set = debug.capabilities().set_variable;
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(area));
    scroll.set_clip_rect(ui.painter().clip_rect().intersect(area));
    egui::ScrollArea::vertical().id_salt("debug-variables").show(&mut scroll, |ui| {
        let mut rows = RowOutcome::default();
        for row in &debug.rows {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(area.width(), ROW), Sense::click());
            show_row(ui, rect, response, row, &mut panel.editing, can_set, "Variable", &mut rows);
        }
        outcome.toggle_row = rows.toggle_row;
        outcome.set_value = rows.set_value;
    });
}

/// One row of a variables tree: the disclosure triangle, the name, the type and the value.
///
/// `editing` is the row being edited and what has been typed into it, passed in rather than read off
/// the tile — the value tooltip has a tree too, and it is the same row drawn the same way.
pub fn show_row(
    ui: &mut egui::Ui,
    rect: Rect,
    response: egui::Response,
    row: &Row,
    editing: &mut Option<(String, String)>,
    can_set: bool,
    what: &str,
    outcome: &mut RowOutcome,
) {
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(Vec2::new(6.0, 2.0)),
            CornerRadius::same(size::CONTROL_CORNER),
            color::control(),
        );
    }
    let left = rect.left() + 12.0 + row.depth as f32 * INDENT;
    if row.has_children() {
        icon::disclosure(
            ui.painter(),
            Pos2::new(left + 6.0, rect.center().y),
            row.expanded,
            color::text_control(),
        );
    }
    let mut pen = left + 16.0;
    let name_tint = match row.is_scope {
        true => color::text_dim(),
        false => color::text(),
    };
    let name = ui.painter().layout_no_wrap(
        row.name.clone(),
        egui::FontId::monospace(11.5),
        name_tint,
    );
    ui.painter().galley(
        Pos2::new(pen, rect.center().y - name.size().y / 2.0),
        name.clone(),
        name_tint,
    );
    pen += name.size().x + 10.0;
    if let Some(kind) = &row.kind {
        let label = ui.painter().layout_no_wrap(
            format!("{kind}"),
            egui::FontId::monospace(10.5),
            color::text_faint(),
        );
        ui.painter().galley(
            Pos2::new(pen, rect.center().y - label.size().y / 2.0),
            label.clone(),
            color::text_faint(),
        );
        pen += label.size().x + 10.0;
    }

    // The cell being edited, which is what `Set Value` turns a row into. `field_text_rect` is what
    // stops it being the sixth field in Quill to put its words against its own top edge.
    if let Some((key, typed)) = editing.as_mut() {
        if *key == row.key {
            let field = Rect::from_min_size(
                Pos2::new(pen, rect.top() + 2.0),
                Vec2::new((rect.right() - pen - 12.0).max(40.0), rect.height() - 4.0),
            );
            ui.painter().rect(
                field,
                CornerRadius::same(size::CONTROL_CORNER),
                color::field(),
                Stroke::new(1.0, color::accent()),
                egui::StrokeKind::Inside,
            );
            let value_id = ui.id().with(("debug-set-value", row.key.clone()));
            let inner = controls::field_takes_the_whole_rectangle(ui, field, 8.0, value_id);
            let editor = ui.put(
                inner,
                egui::TextEdit::singleline(typed)
                    .id(value_id)
                    .frame(egui::Frame::NONE)
                    .desired_width(inner.width())
                    .text_color(color::text_control())
                    .font(egui::FontId::monospace(11.5)),
            );
            editor.request_focus();
            let name = format!("Set {what}: {}", row.name);
            editor.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, &name)
            });
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                outcome.set_value = Some((row.key.clone(), typed.clone()));
                *editing = None;
            } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                *editing = None;
            }
            return;
        }
    }

    if !row.is_scope {
        // A row whose value changed at this stop is tinted, which is the reference editor's change-marking and
        // is what stepping is for.
        let tint = match row.changed {
            true => color::value_changed(),
            false => color::text_control(),
        };
        let value = ui.painter().layout_no_wrap(
            elide(&row.value),
            egui::FontId::monospace(11.5),
            tint,
        );
        ui.painter().galley(
            Pos2::new(pen, rect.center().y - value.size().y / 2.0),
            value,
            tint,
        );
    }

    // `what` is what this row is called where it is drawn — `Variable` in the tile, `Value` in the
    // value tooltip — because **two controls must not share a name**: the same variable is in both
    // trees at once the moment somebody points at a name the tile is already showing.
    let accessible = match row.is_scope {
        true => format!("Scope: {}", row.name),
        false => format!("{what}: {} = {}", row.name, elide(&row.value)),
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, row.expanded, &accessible)
    });
    // A double click on a value turns it into a field; a single click opens the row. Offered only
    // when the adapter said it can change a value, which is the rule every optional control here
    // follows — a control whose capability is absent is absent.
    if response.double_clicked() && can_set && !row.is_scope {
        *editing = Some((row.key.clone(), row.value.clone()));
    } else if response.clicked() && row.has_children() {
        outcome.toggle_row = Some(row.key.clone());
    }
}

/// A pane with nothing in it says why, in the quiet colour and in the middle.
/// What the tile draws with no session: a sentence, and where there is something to press, buttons.
///
/// Three states and one function, because they are three answers to the same question — "why is
/// nothing being debugged" — and a person reads them in the same place.
fn show_idle(ui: &mut egui::Ui, area: Rect, idle: &Idle, outcome: &mut DebugOutcome) {
    let missing = match idle {
        Idle::Ready(message) => return empty(ui, area, message),
        Idle::Building { what, seconds } => {
            return empty(ui, area, &format!("{what}\u{2026} {seconds}s"))
        }
        Idle::Missing(missing) => missing,
    };
    // The sentence, wrapped, sitting above the buttons rather than centred in the whole tile: with
    // something to press underneath it, the pair reads as one thing.
    let text = ui.painter_at(area).layout(
        missing.sentence.clone(),
        egui::FontId::proportional(12.0),
        color::text_faint(),
        (area.width() - 64.0).max(120.0),
    );
    let has_a_command = !missing.install.is_empty();
    let buttons = match has_a_command {
        true => 30.0,
        false => 0.0,
    };
    let size = text.size();
    let top = area.center().y - (size.y + buttons) / 2.0;
    ui.painter_at(area).galley(Pos2::new(area.center().x - size.x / 2.0, top), text, color::text_faint());
    if !has_a_command {
        return;
    }
    // `Install` runs the command in the run tile, where it can be watched; `Copy command` is for
    // somebody who would rather run it themselves, which is a reasonable thing to want of a command
    // that installs software.
    const INSTALL: f32 = 96.0;
    const COPY: f32 = 108.0;
    let row = Pos2::new(area.center().x - (INSTALL + COPY + 6.0) / 2.0, top + size.y + 8.0);
    let install = Rect::from_min_size(row, Vec2::new(INSTALL, 22.0));
    let copy = Rect::from_min_size(Pos2::new(install.right() + 6.0, row.y), Vec2::new(COPY, 22.0));
    if crate::components::modal::button(ui, install, "Install", true, true) {
        outcome.install = Some(missing.adapter.clone());
    }
    if crate::components::modal::button(ui, copy, "Copy command", true, false) {
        outcome.copy = Some(missing.install.clone());
    }
}

fn empty(ui: &egui::Ui, area: Rect, message: &str) {
    let painter = ui.painter_at(area);
    let label = painter.layout(
        message.to_owned(),
        egui::FontId::proportional(12.0),
        color::text_faint(),
        (area.width() - 48.0).max(60.0),
    );
    painter.galley(
        Pos2::new(
            area.center().x - label.size().x / 2.0,
            area.center().y - label.size().y / 2.0,
        ),
        label,
        color::text_faint(),
    );
}

/// An icon button that is dimmed and unclickable when what it does cannot be done just now.
///
/// `run_panel::dimmable_button`'s twin, and it is here rather than shared because that one takes a
/// `fn` pointer and the stepping icons take a parameter. Both dim rather than remove, which is what
/// `A control is absent when it cannot apply` asks for here: stepping applies again the moment the
/// program stops, and a button that came and went under the pointer would be worse than one that
/// waits.
fn dimmable(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    enabled: bool,
    draw: &dyn Fn(&egui::Painter, Pos2),
) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response =
        ui.interact(area, ui.id().with(("debug-button", name)), sense).on_hover_text(name);
    if response.hovered() && enabled {
        ui.painter().rect_filled(area, CornerRadius::same(4), color::control());
    }
    draw(ui.painter(), area.center());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    response.clicked()
}

/// The colour a header button is drawn in, which says whether it can be pressed.
fn tint(enabled: bool) -> egui::Color32 {
    match enabled {
        true => color::text_dim(),
        false => color::text_faint().gamma_multiply(0.6),
    }
}

/// A value that is longer than a row can hold, cut with an ellipsis.
///
/// A row that is a paragraph long pushes every other row off the screen and cannot be read anyway.
/// The whole of it is still what `debug variables` prints, because a command line has no width.
pub fn elide(value: &str) -> String {
    let flat = value.replace('\n', " ");
    match flat.chars().count() > VALUE_LIMIT {
        true => format!("{}\u{2026}", flat.chars().take(VALUE_LIMIT).collect::<String>()),
        false => flat,
    }
}

/// The last part of a path, which is what a frame's row shows.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_is_the_run_tiles_sibling_rather_than_a_second_one_that_resembles_it() {
        assert_eq!(HEADER, crate::components::run_panel::HEADER);
        assert_eq!(HEADER, terminal_panel::HEADER);
    }

    #[test]
    fn a_value_too_long_for_a_row_is_cut_and_a_short_one_is_left_alone() {
        assert_eq!(elide("3"), "3");
        let long: String = std::iter::repeat('x').take(VALUE_LIMIT + 40).collect();
        let cut = elide(&long);
        assert!(cut.ends_with('\u{2026}'));
        assert_eq!(cut.chars().count(), VALUE_LIMIT + 1);
    }

    /// A value with a line break in it would push every row below it down by a line, so it is
    /// flattened rather than drawn as it came.
    #[test]
    fn a_value_with_line_breaks_in_it_stays_on_one_row() {
        assert_eq!(elide("a\nb"), "a b");
    }

    #[test]
    fn a_frames_row_names_the_file_rather_than_the_whole_path() {
        assert_eq!(file_name("C:\\project\\src\\main.rs"), "main.rs");
        assert_eq!(file_name("/project/src/main.rs"), "main.rs");
        assert_eq!(file_name("main.rs"), "main.rs");
    }

    #[test]
    fn a_new_tile_is_put_away_and_editing_nothing() {
        let panel = DebugPanel::new();
        assert!(!panel.visible);
        assert!(panel.editing.is_none());
        assert_eq!(panel.divider, FRAMES_SHARE);
    }
}
