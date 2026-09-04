//! Which side of the window each panel is on, and the arithmetic that turns that into rectangles.
//!
//! `task-1697` asks that a panel be dragged to any edge of the window and snap there, so the shape of
//! the window stopped being a run of `let`s in `UnluminousApp::ui` and became a value. This module is that
//! value and the one function that lays it out; `components::dock` is what a person sees while a panel
//! is in the air, and `tasks/task-1697-panel-docking-tdd.md` is the design.
//!
//! Nothing here draws and nothing here reads the window. [`regions`] takes a rectangle and gives back
//! a rectangle for each panel, so all of it is tested with no window, no graphics card and no fonts.
//!
//! ## The three rules that settle every awkward case
//!
//! **Order is screen order, always along x.** On the left, order 0 is the outermost column; on the
//! right, order 0 is the column nearest the editing area, because on the right "left to right" starts
//! in the middle. In a top or bottom strip the panels are columns as well. One axis everywhere, so
//! "where in this side did the pointer let go" is one comparison — the same one
//! `file_tabs::Strip::position_at` already makes about a tab.
//!
//! **The strips are taken first, across the whole width; the columns come out of what is left.** That
//! is what Unluminous did before any of this: the terminal spans the whole width of the panes, including
//! under the explorer, which is also what the reference editor's bottom tool window does.
//!
//! **A panel carries two measurements and the side decides which is read.** A width for when it is a
//! column at the side, a height for when it is in a strip. One number cannot be both: the terminal is
//! 260 points tall along the bottom, and a 260 point wide column on the right is half a terminal.

use egui::{Pos2, Rect};

use crate::settings::Panes;

/// How many panes a plugin may contribute to one window.
///
/// A number rather than a growing list, so [`Layout`] and [`Regions`] stay `Copy` and allocate nothing:
/// they are built and thrown away several times a frame, once for the real layout and again for each
/// preview of a drop. Four is more panes than the plugins that exist ask for, and a fifth is refused
/// with a message rather than silently dropped, which is the rule every registry in `services::plugins`
/// keeps.
pub const PLUGIN_PANES: usize = 4;

/// How many slots [`Layout`] and [`Regions`] hold: Unluminous's own four, then the plugins'.
pub const SLOTS: usize = 4 + PLUGIN_PANES;

/// The panels that can be moved.
///
/// Unluminous's own four are variants, and the compiler names every place that has to answer for a fifth —
/// the bargain [`crate::app::actions::Action`] and [`crate::app::ViewMode`] already make. A pane a
/// plugin contributed cannot be a variant, because the set is decided when the manifests are read
/// rather than at compile time, so it is a **slot**: `Plugin(0)` is the first contributed pane in
/// `plugins::Surfaces`. Which pane is in which slot is the window's business, and the settings file
/// records a pane's side against its own `<plugin id>/<pane id>` name rather than against a number, so
/// installing a second plugin does not move the first one's pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Explorer,
    Terminal,
    Run,
    Debug,
    Plugin(u8),
}

impl Panel {
    /// Unluminous's own four. A plugin's panes are not in it, because there is no compile time list of them;
    /// [`Panel::all`] is the one that includes them.
    pub const ALL: [Panel; 4] = [Panel::Explorer, Panel::Terminal, Panel::Run, Panel::Debug];

    /// Unluminous's own four, then `contributed` plugin panes.
    ///
    /// What the rail, the dock's menus and `unluminous-cli panels` walk. `contributed` comes from
    /// `plugins::Surfaces`, so a plugin that is switched off is not in it and its pane is gone from
    /// every one of them in the same frame.
    pub fn all(contributed: usize) -> Vec<Panel> {
        let mut found = Panel::ALL.to_vec();
        for slot in 0..contributed.min(PLUGIN_PANES) {
            found.push(Panel::Plugin(slot as u8));
        }
        found
    }

    /// Where this panel sits in the arrays [`Layout`] and [`Panes`] keep.
    pub fn index(self) -> usize {
        match self {
            Panel::Explorer => 0,
            Panel::Terminal => 1,
            Panel::Run => 2,
            Panel::Debug => 3,
            Panel::Plugin(slot) => 4 + (slot as usize).min(PLUGIN_PANES - 1),
        }
    }

    /// The slot number, for a pane a plugin contributed.
    pub fn plugin_slot(self) -> Option<usize> {
        match self {
            Panel::Plugin(slot) => Some(slot as usize),
            _ => None,
        }
    }

    /// The name the command line and the settings file call it: lower case, one word.
    ///
    /// It is also the divider's id, so `Resize explorer` and `Resize terminal` go on meaning what
    /// they meant before any of this — a test asks for a divider by that name.
    pub fn name(self) -> &'static str {
        match self {
            Panel::Explorer => "explorer",
            Panel::Terminal => "terminal",
            Panel::Run => "run",
            Panel::Debug => "debug",
            // A slot's name, used only where a name is needed and no plugin is at hand: a divider's id
            // and a test. What the settings file and the command line call a contributed pane is its
            // `<plugin id>/<pane id>`, which the window resolves — see `Layout::read_from`.
            Panel::Plugin(0) => "plugin-1",
            Panel::Plugin(1) => "plugin-2",
            Panel::Plugin(2) => "plugin-3",
            Panel::Plugin(_) => "plugin-4",
        }
    }

    /// What a person reads: the wording already on the panel's own header and in the rail.
    pub fn label(self) -> &'static str {
        match self {
            // `Project` rather than `Explorer`, because that is what the rail's button is called and
            // no two controls in one window may share a name.
            Panel::Explorer => "Project",
            Panel::Terminal => "Terminal tile",
            Panel::Run => "Run tile",
            Panel::Debug => "Debug tile",
            // A contributed pane's label comes from its manifest, so this is the fallback for a slot
            // with no plugin in it, which nothing draws.
            Panel::Plugin(_) => "Plugin pane",
        }
    }

    pub fn from_name(name: &str) -> Option<Panel> {
        Panel::all(PLUGIN_PANES).into_iter().find(|panel| panel.name() == name)
    }

    /// True for the three panels that draw a character grid.
    ///
    /// It is what `task-1683`'s rule is really about: two grids in one strip are two half-sized
    /// grids, so a side shows one of these at a time. The explorer is a list and never competes.
    pub fn is_a_tile(self) -> bool {
        !matches!(self, Panel::Explorer | Panel::Plugin(_))
    }

    /// The same question, told what the plugins' manifests said.
    ///
    /// A contributed pane is a tile when its `pane.group` is `bottom`, which is the rail's own distinction:
    /// the bottom group holds the things with a character grid in them, and two grids stacked in one strip
    /// are two half sized grids. `is_a_tile` cannot answer for a plugin because the answer is in a manifest,
    /// so every caller that has the surfaces to hand asks this one instead.
    pub fn is_a_tile_given(self, tiles: &[bool]) -> bool {
        match self {
            Panel::Plugin(slot) => tiles.get(slot as usize).copied().unwrap_or(false),
            other => other.is_a_tile(),
        }
    }
}

/// The edge a panel is docked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "Left",
            Side::Right => "Right",
            Side::Top => "Top",
            Side::Bottom => "Bottom",
        }
    }

    pub fn from_name(name: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|side| side.name() == name)
    }

    /// True for the two sides whose panels are columns of their own width: left and right.
    ///
    /// The other two are strips across the whole width, where a panel's **height** is what is read
    /// and the last column fills what is left over.
    pub fn is_a_column(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }

    /// True for the two sides that are taken across the whole width.
    ///
    /// The other half of [`Self::is_a_column`], written out because "not a column" reads as a denial
    /// where "a strip" is the thing itself, and because the rule that one tile shows at a time is a rule
    /// about a strip.
    pub fn is_a_strip(self) -> bool {
        !self.is_a_column()
    }

    /// The side facing this one across the window.
    ///
    /// Which is who a divider takes room **from** when there is no editing area between the two — see
    /// [`fill_the_depth`] and `UnluminousApp::move_a_divider_with_no_editor`.
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

/// Which side each panel is on, and where in that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    sides: [Side; SLOTS],
    orders: [usize; SLOTS],
    /// How many of the plugin slots hold a pane.
    ///
    /// A slot with nothing in it is **not a panel on any side**: it is not in `panels_on`, it takes no
    /// room and no divider is drawn for it. Without this the default layout would have four panes on the
    /// right that nobody contributed. Set by the window from `plugins::Surfaces` when the plugins are
    /// read, so switching a plugin off takes its pane out of the layout in the same frame.
    plugin_panes: u8,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    /// The arrangement Unluminous has always had: the explorer down the left, the three tiles along the
    /// bottom in the order the rail lists them.
    pub fn new() -> Self {
        let mut sides = [Side::Right; SLOTS];
        sides[0] = Side::Left;
        sides[1] = Side::Bottom;
        sides[2] = Side::Bottom;
        sides[3] = Side::Bottom;
        let mut orders = [0; SLOTS];
        orders[2] = 1;
        orders[3] = 2;
        Self { sides, orders, plugin_panes: 0 }
    }

    /// How many panes the plugins that are switched on contribute.
    pub fn plugin_panes(&self) -> usize {
        self.plugin_panes as usize
    }

    /// Say how many contributed panes there are, and where each one asked to be docked.
    ///
    /// `sides` is what the manifests said, and it is used only for a slot the settings file has not
    /// spoken about: once somebody has dragged a pane, where they left it wins over what its manifest
    /// asked for, which is the rule every panel already follows.
    pub fn set_plugin_panes(&mut self, sides: &[Side], already_placed: &[bool]) {
        self.plugin_panes = (sides.len().min(PLUGIN_PANES)) as u8;
        for (slot, side) in sides.iter().enumerate().take(PLUGIN_PANES) {
            if !already_placed.get(slot).copied().unwrap_or(false) {
                self.sides[Panel::Plugin(slot as u8).index()] = *side;
            }
        }
        for side in Side::ALL {
            self.tidy(side);
        }
    }

    pub fn side_of(&self, panel: Panel) -> Side {
        self.sides[panel.index()]
    }

    pub fn order_of(&self, panel: Panel) -> usize {
        self.orders[panel.index()]
    }

    /// The panels docked to `side`, in screen order.
    pub fn panels_on(&self, side: Side) -> Vec<Panel> {
        let mut found: Vec<Panel> = Panel::all(self.plugin_panes as usize)
            .into_iter()
            .filter(|panel| self.side_of(*panel) == side)
            .collect();
        found.sort_by_key(|panel| self.order_of(*panel));
        found
    }

    /// Move `panel` to `side`. `position` is where in that side, counting the panels already there
    /// **without** this one, so it is a plain insertion index; `None` puts it at the end.
    pub fn dock(&mut self, panel: Panel, side: Side, position: Option<usize>) {
        let others: Vec<Panel> =
            self.panels_on(side).into_iter().filter(|other| *other != panel).collect();
        let at = position.unwrap_or(others.len()).min(others.len());
        let from = self.side_of(panel);
        self.sides[panel.index()] = side;
        // Everyone already there gets an odd number and the newcomer the even one that puts it where
        // it was asked for; `tidy` then squashes them back down to `0..n`. Simpler than shuffling
        // each neighbour by hand, and it cannot leave two panels sharing an order.
        for (index, other) in others.iter().enumerate() {
            self.orders[other.index()] = index * 2 + 1;
        }
        self.orders[panel.index()] = at * 2;
        self.tidy(side);
        if from != side {
            self.tidy(from);
        }
    }

    /// The same move, on a copy. This is what the preview is painted from: the layout as it would be
    /// after the drop, laid out by the same [`regions`] that will lay it out for real.
    pub fn with(&self, panel: Panel, side: Side, position: Option<usize>) -> Layout {
        let mut copy = *self;
        copy.dock(panel, side, position);
        copy
    }

    /// Put every panel back where it started.
    pub fn reset(&mut self) {
        *self = Layout::new();
    }

    /// Renumber one side `0..n`, keeping the order the panels are already in.
    ///
    /// The two invariants this keeps are `OpenFiles::tidy`'s two, restated: no gaps, and no two
    /// panels on one side sharing an order.
    fn tidy(&mut self, side: Side) {
        for (index, panel) in self.panels_on(side).into_iter().enumerate() {
            self.orders[panel.index()] = index;
        }
    }

    /// Read the arrangement out of the settings file, falling back to the default for anything it
    /// does not name — so a file written by an older Unluminous opens as the layout that Unluminous had.
    pub fn read_from(values: &crate::services::store::Values) -> Layout {
        Layout::read_from_with(values, &[])
    }

    /// The same, with the names of the panes plugins contributed, in slot order.
    ///
    /// A contributed pane's side is recorded against its own `<plugin id>/<pane id>` rather than against
    /// its slot number, so installing a second plugin does not move the first one's pane to wherever the
    /// second one's was left.
    pub fn read_from_with(
        values: &crate::services::store::Values,
        plugin_panes: &[String],
    ) -> Layout {
        let mut layout = Layout::read_built_in(values);
        for (slot, key) in plugin_panes.iter().enumerate().take(PLUGIN_PANES) {
            let panel = Panel::Plugin(slot as u8);
            if let Some(name) = values.text(&format!("panes.{key}.side")) {
                if let Some(side) = Side::from_name(name.trim()) {
                    layout.sides[panel.index()] = side;
                }
            }
            if let Some(order) = values.number(&format!("panes.{key}.order")) {
                layout.orders[panel.index()] = order.max(0.0) as usize;
            }
        }
        for side in Side::ALL {
            layout.tidy(side);
        }
        layout
    }

    fn read_built_in(values: &crate::services::store::Values) -> Layout {
        let mut layout = Layout::new();
        for panel in Panel::ALL {
            if let Some(name) = values.text(&format!("panes.{}.side", panel.name())) {
                if let Some(side) = Side::from_name(name.trim()) {
                    layout.sides[panel.index()] = side;
                }
            }
            if let Some(order) = values.number(&format!("panes.{}.order", panel.name())) {
                layout.orders[panel.index()] = order.max(0.0) as usize;
            }
        }
        for side in Side::ALL {
            layout.tidy(side);
        }
        layout
    }

    pub fn write_into(&self, values: &mut crate::services::store::Values) {
        self.write_into_with(values, &[]);
    }

    /// The same, with the names of the panes plugins contributed, in slot order.
    pub fn write_into_with(
        &self,
        values: &mut crate::services::store::Values,
        plugin_panes: &[String],
    ) {
        for panel in Panel::ALL {
            values.set(&format!("panes.{}.side", panel.name()), self.side_of(panel).name());
            values.set(&format!("panes.{}.order", panel.name()), self.order_of(panel).to_string());
        }
        for (slot, key) in plugin_panes.iter().enumerate().take(PLUGIN_PANES) {
            let panel = Panel::Plugin(slot as u8);
            values.set(&format!("panes.{key}.side"), self.side_of(panel).name());
            values.set(&format!("panes.{key}.order"), self.order_of(panel).to_string());
        }
    }
}

/// The narrowest the editing area is ever left, in points.
///
/// [`crate::theme::size::EDITOR_PANE_MIN`] is what a single editing pane may be dragged down to, and
/// the panels either side of it are held to the same promise.
pub const EDITOR_MIN_WIDTH: f32 = crate::theme::size::EDITOR_PANE_MIN;

/// The shortest the editing area is ever left.
///
/// 120 points, which is the number the three tiles have always been clamped against — `(panes.height()
/// - 120).max(MIN)` was written out three times in `UnluminousApp::ui` and is now written once.
pub const EDITOR_MIN_HEIGHT: f32 = 120.0;

/// Where every panel that is showing was put, and what is left for the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Regions {
    /// One rectangle per panel, indexed by [`Panel::index`]. [`Rect::ZERO`] for a panel that is not
    /// showing. The first four are Unluminous's own and the rest are the plugin slots.
    pub panels: [Rect; SLOTS],
    pub editor: Rect,
}

impl Regions {
    pub fn of(&self, panel: Panel) -> Rect {
        self.panels[panel.index()]
    }
}

/// Lay the panels out inside `body`.
///
/// `showing` says which panels are drawn at all, indexed by [`Panel::index`]. A panel that is not
/// showing takes no room and gets [`Rect::ZERO`], which is what makes a slot with no plugin in it cost
/// nothing: nothing is showing there, so nothing is laid out for it.
pub fn regions(body: Rect, layout: &Layout, showing: [bool; SLOTS], sizes: &Panes) -> Regions {
    regions_with(body, layout, showing, sizes, true)
}

/// The same, told whether the editing area is showing.
///
/// `task-28` asks for a toggle that hides the pane holding the tabs. When it is hidden the panels take the whole
/// width and the whole height: [`EDITOR_MIN_WIDTH`] and [`EDITOR_MIN_HEIGHT`] are what is kept **for the editing
/// area**, so with no editing area there is nothing to keep, and [`Regions::editor`] is [`Rect::ZERO`].
///
/// Nothing else about the arithmetic changes. The strips are still taken first across the whole width and the
/// columns still come out of what is left, so a terminal along the bottom with the editing area hidden is a
/// terminal along the bottom with the explorer above it, which is what dragging it there already means.
pub fn regions_with(
    body: Rect,
    layout: &Layout,
    showing: [bool; SLOTS],
    sizes: &Panes,
    editor: bool,
) -> Regions {
    let (keep_width, keep_height) = match editor {
        true => (EDITOR_MIN_WIDTH, EDITOR_MIN_HEIGHT),
        false => (0.0, 0.0),
    };
    let mut panels = [Rect::ZERO; SLOTS];
    let visible = |side: Side| -> Vec<Panel> {
        layout.panels_on(side).into_iter().filter(|panel| showing[panel.index()]).collect()
    };

    // The two strips, taken across the whole width. Their depth is the greatest of the heights of
    // the panels in them, because a strip is one band and a panel in it that wanted less would leave
    // a hole under the one that wanted more.
    let top_panels = visible(Side::Top);
    let bottom_panels = visible(Side::Bottom);
    let depth = |panels: &[Panel]| -> f32 {
        panels.iter().map(|panel| sizes.height_of(*panel)).fold(0.0_f32, f32::max)
    };
    let (top_depth, bottom_depth) = match editor {
        true => share_the_depth(body.height(), depth(&top_panels), depth(&bottom_panels), keep_height),
        // With no editing area there is nothing between the two strips, so they fill the height between them
        // rather than each taking its own and leaving a gap.
        false => fill_the_depth(body.height(), depth(&top_panels), depth(&bottom_panels)),
    };

    let top_strip = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + top_depth));
    let bottom_strip =
        Rect::from_min_max(Pos2::new(body.left(), body.bottom() - bottom_depth), body.max);
    let middle = Rect::from_min_max(
        Pos2::new(body.left(), top_strip.bottom()),
        Pos2::new(body.right(), bottom_strip.top()),
    );

    lay_a_strip_out(top_strip, &top_panels, sizes, &mut panels);
    lay_a_strip_out(bottom_strip, &bottom_panels, sizes, &mut panels);

    // Then the columns, out of what the strips left. Each takes its own width, so a side's depth is
    // the sum of its columns rather than the greatest of them.
    let left_panels = visible(Side::Left);
    let right_panels = visible(Side::Right);
    let width = |panels: &[Panel]| -> f32 {
        panels.iter().map(|panel| sizes.width_of(*panel)).sum::<f32>()
    };
    let (left_depth, right_depth) = match editor {
        true => share_the_depth(middle.width(), width(&left_panels), width(&right_panels), keep_width),
        false => fill_the_depth(middle.width(), width(&left_panels), width(&right_panels)),
    };

    let left_region =
        Rect::from_min_max(middle.min, Pos2::new(middle.left() + left_depth, middle.bottom()));
    let right_region =
        Rect::from_min_max(Pos2::new(middle.right() - right_depth, middle.top()), middle.max);
    lay_columns_out(left_region, &left_panels, sizes, &mut panels);
    lay_columns_out(right_region, &right_panels, sizes, &mut panels);

    // **`Rect::ZERO` when it is hidden**, which is what a panel that is not showing gets and what every reader
    // of a rectangle in this module already treats as "not there".
    let editor = match editor {
        true => Rect::from_min_max(
            Pos2::new(left_region.right(), middle.top()),
            Pos2::new(right_region.left(), middle.bottom()),
        ),
        false => Rect::ZERO,
    };
    Regions { panels, editor }
}

/// How much of `room` the two opposite sides get when there is nothing between them to leave space for.
///
/// `task-28`: hiding the editing area has to give the room to the panels, not leave a gap where it used to be. So
/// the two sides are scaled **in proportion to what they asked for** until they fill `room` — which for the
/// common case, one side with panels and the other with none, means that side takes all of it.
///
/// Two sides asking for nothing get nothing, and `regions_with` is only asked this when at least one panel is
/// showing: `Action::ToggleEditor` will not hide the editing area with an empty window behind it.
fn fill_the_depth(room: f32, first: f32, second: f32) -> (f32, f32) {
    let wanted = first + second;
    if wanted <= 0.0 || room <= 0.0 {
        return (0.0, 0.0);
    }
    let scale = room / wanted;
    let taken = first * scale;
    // The second gets the remainder rather than its own scaled share, so the two always add up to exactly
    // `room` however the multiplication rounds.
    (taken, room - taken)
}

/// How much of `room` the two opposite sides get, leaving at least `keep` in the middle.
///
/// They are shrunk **in proportion** when they ask for more than there is, which is what makes the
/// single-strip case come out at exactly the number `UnluminousApp::ui` used to clamp against: one strip
/// asking for more than `room - keep` gets `room - keep`.
fn share_the_depth(room: f32, first: f32, second: f32, keep: f32) -> (f32, f32) {
    let spare = (room - keep).max(0.0);
    let want = first + second;
    if want <= spare || want <= 0.0 {
        return (first, second);
    }
    let scale = spare / want;
    (first * scale, second * scale)
}

/// A strip across the window: the panels are columns left to right, each its own width, and the last
/// one fills what is left.
///
/// That is what makes the explorer dropped beside the terminal along the bottom come out as 248
/// points of file tree with the terminal filling the rest, using the number the explorer already has.
fn lay_a_strip_out(strip: Rect, order: &[Panel], sizes: &Panes, out: &mut [Rect; SLOTS]) {
    if order.is_empty() || strip.height() <= 0.0 {
        return;
    }
    let wants: Vec<f32> = order.iter().map(|panel| sizes.width_of(*panel)).collect();
    let mins: Vec<f32> = order.iter().map(|panel| sizes.min_width_of(*panel)).collect();
    let mut pen = strip.left();
    for (index, panel) in order.iter().enumerate() {
        let width = if index + 1 == order.len() {
            (strip.right() - pen).max(0.0)
        } else {
            let rest: f32 = mins[index + 1..].iter().sum();
            wants[index].max(mins[index]).min((strip.right() - pen - rest).max(0.0))
        };
        out[panel.index()] =
            Rect::from_min_max(Pos2::new(pen, strip.top()), Pos2::new(pen + width, strip.bottom()));
        pen += width;
    }
}

/// A side of the window: the panels are columns left to right in order, each its own width, and the
/// region is exactly as wide as they add up to.
fn lay_columns_out(region: Rect, order: &[Panel], sizes: &Panes, out: &mut [Rect; SLOTS]) {
    if order.is_empty() || region.width() <= 0.0 {
        return;
    }
    // The region may have been shrunk to leave the editing area its minimum, so the columns are
    // scaled by whatever fraction of what they asked for they actually got.
    let wanted: f32 = order.iter().map(|panel| sizes.width_of(*panel)).sum();
    let scale = if wanted > 0.0 { (region.width() / wanted).min(1.0) } else { 0.0 };
    let mut pen = region.left();
    for (index, panel) in order.iter().enumerate() {
        let width = if index + 1 == order.len() {
            (region.right() - pen).max(0.0)
        } else {
            (sizes.width_of(*panel) * scale).min((region.right() - pen).max(0.0))
        };
        out[panel.index()] = Rect::from_min_max(
            Pos2::new(pen, region.top()),
            Pos2::new(pen + width, region.bottom()),
        );
        pen += width;
    }
}

// ------------------------------------------------------------------------------- the drop targets

/// How deep a drop band is when the side it belongs to holds nothing yet.
pub const ZONE: f32 = 72.0;

/// The most of the window one band may take, so the middle always exists.
const ZONE_SHARE: f32 = 0.4;

/// One place a panel can be let go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub side: Side,
    /// Where the pointer has to be for this side to be the answer.
    pub band: Rect,
}

/// The four bands, one along each edge of `body`.
///
/// A band is [`ZONE`] deep, or as deep as whatever is already docked to that side if that is deeper —
/// which is what lets the pointer reach *past the middle* of a panel that is already there and so
/// choose to land after it rather than before it, with no second control and no modifier key.
pub fn zones(body: Rect, layout: &Layout, showing: [bool; SLOTS], sizes: &Panes) -> [Zone; 4] {
    let placed = regions(body, layout, showing, sizes);
    let occupied = |side: Side| -> f32 {
        layout
            .panels_on(side)
            .into_iter()
            .filter(|panel| showing[panel.index()])
            .map(|panel| match side {
                Side::Left => placed.of(panel).right() - body.left(),
                Side::Right => body.right() - placed.of(panel).left(),
                Side::Top => placed.of(panel).bottom() - body.top(),
                Side::Bottom => body.bottom() - placed.of(panel).top(),
            })
            .fold(0.0_f32, f32::max)
    };
    Side::ALL.map(|side| {
        let room = if side.is_a_column() { body.width() } else { body.height() };
        let depth = occupied(side).max(ZONE).min(room * ZONE_SHARE).max(0.0);
        let band = match side {
            Side::Left => {
                Rect::from_min_max(body.min, Pos2::new(body.left() + depth, body.bottom()))
            }
            Side::Right => {
                Rect::from_min_max(Pos2::new(body.right() - depth, body.top()), body.max)
            }
            Side::Top => Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + depth)),
            Side::Bottom => {
                Rect::from_min_max(Pos2::new(body.left(), body.bottom() - depth), body.max)
            }
        };
        Zone { side, band }
    })
}

/// Which side the pointer is aiming at, and where in that side the panel would land.
///
/// A point inside two bands belongs to the one it is **deeper into**, measured as a fraction of that
/// band's own depth, so a corner goes to the edge it is nearest — which is what somebody aiming at a
/// corner means. Outside every band there is no answer, and letting go there leaves the panel where
/// it was: the "a drag can be thought better of" rule the explorer's row drag and the tab drag both
/// already state.
pub fn target(
    body: Rect,
    layout: &Layout,
    showing: [bool; SLOTS],
    sizes: &Panes,
    carrying: Panel,
    pointer: Pos2,
) -> Option<(Side, usize)> {
    let bands = zones(body, layout, showing, sizes);
    let mut best: Option<(Side, f32)> = None;
    for zone in bands {
        if !zone.band.contains(pointer) {
            continue;
        }
        let (depth, into) = match zone.side {
            Side::Left => (zone.band.width(), pointer.x - zone.band.left()),
            Side::Right => (zone.band.width(), zone.band.right() - pointer.x),
            Side::Top => (zone.band.height(), pointer.y - zone.band.top()),
            Side::Bottom => (zone.band.height(), zone.band.bottom() - pointer.y),
        };
        // 1.0 at the window's own edge and 0.0 at the inner edge of the band, so the outer side wins
        // a corner.
        let penetration = if depth > 0.0 { 1.0 - (into / depth) } else { 0.0 };
        if best.is_none_or(|(_, best)| penetration > best) {
            best = Some((zone.side, penetration));
        }
    }
    let (side, _) = best?;
    Some((side, position_in(body, layout, showing, sizes, side, carrying, pointer.x)))
}

/// Where along `side` the pointer is: **after every panel whose middle it has passed**.
///
/// `file_tabs::Strip::position_at`'s rule, word for word, and the reason order is screen order along
/// x on every side — one comparison answers it for all four. The panel being carried is left out, so
/// what comes back is a plain insertion index and no caller has to subtract one.
pub fn position_in(
    body: Rect,
    layout: &Layout,
    showing: [bool; SLOTS],
    sizes: &Panes,
    side: Side,
    carrying: Panel,
    x: f32,
) -> usize {
    let placed = regions(body, layout, showing, sizes);
    layout
        .panels_on(side)
        .into_iter()
        .filter(|panel| *panel != carrying && showing[panel.index()])
        .filter(|panel| x > placed.of(*panel).center().x)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Rect {
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 700.0))
    }

    fn all_showing() -> [bool; SLOTS] {
        // Unluminous's own four. A slot with no plugin in it is never showing, which is what makes the
        // arithmetic below the same arithmetic it was before plugins could contribute a pane.
        only(&Panel::ALL)
    }

    fn only(panels: &[Panel]) -> [bool; SLOTS] {
        let mut showing = [false; SLOTS];
        for panel in panels {
            showing[panel.index()] = true;
        }
        showing
    }

    #[test]
    fn the_default_is_the_window_unluminous_has_always_had() {
        let layout = Layout::new();
        assert_eq!(layout.side_of(Panel::Explorer), Side::Left);
        assert_eq!(layout.panels_on(Side::Bottom), vec![Panel::Terminal, Panel::Run, Panel::Debug]);
        assert!(layout.panels_on(Side::Right).is_empty());
        assert!(layout.panels_on(Side::Top).is_empty());
    }

    #[test]
    fn a_side_is_always_numbered_from_zero_with_no_gaps() {
        let mut layout = Layout::new();
        layout.dock(Panel::Terminal, Side::Right, None);
        for side in Side::ALL {
            let orders: Vec<usize> =
                layout.panels_on(side).into_iter().map(|panel| layout.order_of(panel)).collect();
            assert_eq!(orders, (0..orders.len()).collect::<Vec<_>>(), "{side:?} is not tidy");
        }
    }

    #[test]
    fn a_panel_can_be_put_before_or_after_the_one_already_there() {
        let mut layout = Layout::new();
        layout.dock(Panel::Terminal, Side::Left, Some(1));
        assert_eq!(layout.panels_on(Side::Left), vec![Panel::Explorer, Panel::Terminal]);
        layout.dock(Panel::Terminal, Side::Left, Some(0));
        assert_eq!(layout.panels_on(Side::Left), vec![Panel::Terminal, Panel::Explorer]);
    }

    #[test]
    fn docking_a_panel_where_it_already_is_changes_nothing() {
        let before = Layout::new();
        let mut after = before;
        after.dock(Panel::Run, Side::Bottom, Some(1));
        assert_eq!(before, after);
    }

    #[test]
    fn three_panels_on_one_side_keep_their_order_when_the_middle_one_leaves() {
        let mut layout = Layout::new();
        layout.dock(Panel::Run, Side::Top, None);
        assert_eq!(layout.panels_on(Side::Bottom), vec![Panel::Terminal, Panel::Debug]);
        assert_eq!(layout.order_of(Panel::Debug), 1);
    }

    #[test]
    fn the_default_layout_is_the_arithmetic_the_window_used_to_do_inline() {
        // The explorer down the left at its width, the terminal across the whole bottom at its
        // height, and the editing area filling the rest — which is what `UnluminousApp::ui` spelled out
        // before this module existed.
        let sizes = Panes::new();
        let placed = regions(body(), &Layout::new(), only(&[Panel::Explorer, Panel::Terminal]), &sizes);
        let explorer = placed.of(Panel::Explorer);
        let terminal = placed.of(Panel::Terminal);
        assert_eq!(explorer.left(), 0.0);
        assert_eq!(explorer.width(), sizes.explorer_width);
        assert_eq!(terminal.width(), 1000.0, "the strip takes the whole width, under the explorer");
        assert_eq!(terminal.height(), sizes.terminal_height);
        assert_eq!(explorer.bottom(), terminal.top(), "the explorer stops where the terminal starts");
        assert_eq!(placed.editor.left(), explorer.right());
        assert_eq!(placed.editor.right(), 1000.0);
        assert_eq!(placed.editor.bottom(), terminal.top());
    }

    #[test]
    fn a_panel_docked_right_is_a_column_of_its_own_width_down_the_right_hand_edge() {
        let sizes = Panes::new();
        let layout = Layout::new().with(Panel::Terminal, Side::Right, None);
        let placed = regions(body(), &layout, only(&[Panel::Explorer, Panel::Terminal]), &sizes);
        let terminal = placed.of(Panel::Terminal);
        assert_eq!(terminal.right(), 1000.0);
        assert_eq!(terminal.width(), sizes.terminal_width);
        assert_eq!(terminal.height(), 700.0, "a column at the side is as tall as the body");
        assert_eq!(placed.editor.right(), terminal.left());
    }

    #[test]
    fn two_panels_on_the_left_sit_side_by_side_in_order() {
        let sizes = Panes::new();
        let layout = Layout::new().with(Panel::Terminal, Side::Left, Some(1));
        let placed = regions(body(), &layout, only(&[Panel::Explorer, Panel::Terminal]), &sizes);
        let explorer = placed.of(Panel::Explorer);
        let terminal = placed.of(Panel::Terminal);
        assert_eq!(explorer.left(), 0.0);
        assert_eq!(explorer.right(), terminal.left(), "no gap between the two columns");
        assert_eq!(terminal.right(), placed.editor.left());
        assert_eq!(explorer.width() + terminal.width(), sizes.explorer_width + sizes.terminal_width);
    }

    #[test]
    fn on_the_right_the_first_column_is_the_one_nearest_the_document() {
        let sizes = Panes::new();
        let mut layout = Layout::new();
        layout.dock(Panel::Terminal, Side::Right, Some(0));
        layout.dock(Panel::Run, Side::Right, Some(1));
        let placed = regions(body(), &layout, only(&[Panel::Terminal, Panel::Run]), &sizes);
        assert!(
            placed.of(Panel::Terminal).left() < placed.of(Panel::Run).left(),
            "order is screen order along x, on every side"
        );
        assert_eq!(placed.of(Panel::Run).right(), 1000.0);
    }

    #[test]
    fn a_strip_gives_the_last_panel_in_it_whatever_is_left() {
        let sizes = Panes::new();
        let layout = Layout::new().with(Panel::Explorer, Side::Bottom, Some(0));
        let placed = regions(body(), &layout, only(&[Panel::Explorer, Panel::Terminal]), &sizes);
        assert_eq!(placed.of(Panel::Explorer).width(), sizes.explorer_width);
        assert_eq!(placed.of(Panel::Terminal).right(), 1000.0);
        assert_eq!(placed.of(Panel::Terminal).left(), placed.of(Panel::Explorer).right());
        assert_eq!(placed.editor.height(), 700.0 - sizes.terminal_height);
    }

    #[test]
    fn the_document_is_never_squeezed_out_however_much_the_panels_ask_for() {
        let mut sizes = Panes::new();
        sizes.explorer_width = 900.0;
        sizes.terminal_width = 900.0;
        sizes.terminal_height = 900.0;
        let layout = Layout::new().with(Panel::Terminal, Side::Right, None);
        let placed = regions(body(), &layout, only(&[Panel::Explorer, Panel::Terminal]), &sizes);
        assert!(placed.editor.width() >= EDITOR_MIN_WIDTH - 0.01, "{:?}", placed.editor);
        assert!(placed.editor.height() >= EDITOR_MIN_HEIGHT - 0.01, "{:?}", placed.editor);
    }

    #[test]
    fn a_strip_asking_for_the_whole_window_leaves_the_document_its_minimum() {
        // The number `UnluminousApp::ui` used to clamp against, written once now.
        let mut sizes = Panes::new();
        sizes.terminal_height = 5000.0;
        let placed = regions(body(), &Layout::new(), only(&[Panel::Terminal]), &sizes);
        assert!((placed.of(Panel::Terminal).height() - (700.0 - EDITOR_MIN_HEIGHT)).abs() < 0.01);
    }

    #[test]
    fn a_panel_that_is_not_showing_takes_no_room_at_all() {
        let sizes = Panes::new();
        let placed = regions(body(), &Layout::new(), only(&[Panel::Terminal]), &sizes);
        assert_eq!(placed.of(Panel::Explorer), Rect::ZERO);
        assert_eq!(placed.editor.left(), 0.0);
    }

    #[test]
    fn the_four_bands_cover_the_four_edges_and_the_middle_is_no_target() {
        let sizes = Panes::new();
        let layout = Layout::new();
        let showing = all_showing();
        let bands = zones(body(), &layout, showing, &sizes);
        for zone in bands {
            assert!(zone.band.width() > 0.0 && zone.band.height() > 0.0);
        }
        assert_eq!(
            target(body(), &layout, showing, &sizes, Panel::Terminal, Pos2::new(500.0, 350.0)),
            None,
            "the document is not a dock host"
        );
    }

    #[test]
    fn a_band_reaches_past_what_is_already_docked_to_its_side() {
        // Without this the pointer could never get past the explorer's middle, and "to the right of
        // the side panel" would be unreachable.
        let sizes = Panes::new();
        let layout = Layout::new();
        let bands = zones(body(), &layout, only(&[Panel::Explorer]), &sizes);
        let left = bands.iter().find(|zone| zone.side == Side::Left).expect("a left band");
        assert!(left.band.right() >= sizes.explorer_width, "{:?}", left.band);
    }

    #[test]
    fn a_corner_goes_to_the_edge_it_is_nearest() {
        let sizes = Panes::new();
        let layout = Layout::new();
        let showing = only(&[Panel::Explorer]);
        // Two points in the bottom left corner: one hard against the left edge, one hard against the
        // bottom.
        let (side, _) =
            target(body(), &layout, showing, &sizes, Panel::Run, Pos2::new(2.0, 660.0)).expect("a side");
        assert_eq!(side, Side::Left);
        let (side, _) =
            target(body(), &layout, showing, &sizes, Panel::Run, Pos2::new(60.0, 699.0)).expect("a side");
        assert_eq!(side, Side::Bottom);
    }

    #[test]
    fn where_in_the_side_it_lands_is_which_side_of_the_explorers_middle_the_pointer_is() {
        let sizes = Panes::new();
        let layout = Layout::new();
        let showing = only(&[Panel::Explorer, Panel::Terminal]);
        let middle = sizes.explorer_width / 2.0;
        let before = target(body(), &layout, showing, &sizes, Panel::Terminal, Pos2::new(middle - 20.0, 300.0));
        assert_eq!(before, Some((Side::Left, 0)));
        let after = target(body(), &layout, showing, &sizes, Panel::Terminal, Pos2::new(middle + 20.0, 300.0));
        assert_eq!(after, Some((Side::Left, 1)));
    }

    #[test]
    fn the_preview_is_the_layout_rather_than_a_guess_about_it() {
        // What the blue rectangle is painted from, and what the drop then does, are one function
        // applied to one value — so they cannot disagree.
        let sizes = Panes::new();
        let mut layout = Layout::new();
        let showing = only(&[Panel::Explorer, Panel::Terminal]);
        let preview = regions(body(), &layout.with(Panel::Terminal, Side::Right, Some(0)), showing, &sizes);
        layout.dock(Panel::Terminal, Side::Right, Some(0));
        let after = regions(body(), &layout, showing, &sizes);
        assert_eq!(preview, after);
    }

    #[test]
    fn the_arrangement_survives_being_written_and_read_back() {
        let mut layout = Layout::new();
        layout.dock(Panel::Terminal, Side::Left, Some(0));
        layout.dock(Panel::Debug, Side::Right, None);
        let mut values = crate::services::store::Values::new();
        layout.write_into(&mut values);
        assert_eq!(Layout::read_from(&values), layout);
    }

    #[test]
    fn a_settings_file_written_before_any_of_this_reads_as_the_window_unluminous_had() {
        let values = crate::services::store::Values::new();
        assert_eq!(Layout::read_from(&values), Layout::new());
    }

    #[test]
    fn every_panel_and_every_side_reads_back_from_its_name() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_name(panel.name()), Some(panel));
        }
        for side in Side::ALL {
            assert_eq!(Side::from_name(side.name()), Some(side));
        }
    }

    /// `task-28`: a toggle under the folder icon hides the pane holding the tabs.
    ///
    /// With no editing area there is nothing to keep room for, so the panels take the whole width. The default
    /// layout puts the explorer down the left, so this is the explorer filling the window.
    #[test]
    fn hiding_the_editing_area_gives_the_whole_width_to_the_panels() {
        let body = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 700.0));
        let layout = Layout::default();
        let sizes = Panes::default();
        let mut showing = [false; SLOTS];
        showing[Panel::Explorer.index()] = true;

        let with = regions_with(body, &layout, showing, &sizes, true);
        assert_eq!(with.of(Panel::Explorer).width(), sizes.explorer_width, "it asks for its own width");
        assert!(with.editor.width() > 0.0, "and the editing area has what is left");

        let without = regions_with(body, &layout, showing, &sizes, false);
        assert_eq!(without.editor, Rect::ZERO, "a hidden editing area takes no room, like a hidden panel");
        assert_eq!(without.of(Panel::Explorer).width(), 1000.0, "so the explorer has the whole width");
        assert_eq!(without.of(Panel::Explorer).height(), 700.0, "and the whole height");
    }

    /// The same for a strip: a terminal along the bottom takes the whole height with nothing above it to keep
    /// room for. What does not change is the arrangement — the strip is still taken across the whole width.
    #[test]
    fn hiding_the_editing_area_gives_a_strip_the_whole_height() {
        let body = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 700.0));
        let layout = Layout::default();
        let sizes = Panes::default();
        let mut showing = [false; SLOTS];
        showing[Panel::Terminal.index()] = true;

        let with = regions_with(body, &layout, showing, &sizes, true);
        assert_eq!(with.of(Panel::Terminal).height(), sizes.terminal_height);

        let without = regions_with(body, &layout, showing, &sizes, false);
        assert_eq!(without.of(Panel::Terminal).height(), 700.0, "the terminal takes the whole height");
        assert_eq!(without.of(Panel::Terminal).width(), 1000.0, "and still spans the whole width");
        assert_eq!(without.editor, Rect::ZERO);
    }

    /// `regions` is `regions_with` with the editing area showing, which is what every existing caller and every
    /// existing test in this module means.
    #[test]
    fn the_older_reader_means_the_editing_area_is_showing() {
        let body = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 700.0));
        let layout = Layout::default();
        let sizes = Panes::default();
        let showing = [true; SLOTS];
        assert_eq!(
            regions(body, &layout, showing, &sizes),
            regions_with(body, &layout, showing, &sizes, true)
        );
    }
}
