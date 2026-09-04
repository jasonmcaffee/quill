//! What the Settings window holds, and what is remembered between runs.
//!
//! Two structures, because they are two different things. [`Settings`] is what a person chooses in
//! `Edit -> Settings`: the editor's font, the background opacity and the terminal's font size.
//! [`Panes`] is where the draggable dividers were left, which nobody chooses in a dialog but which should
//! still be there next time — and, since `task-1697`, which edge of the window each panel was left
//! docked to. Both are the same kind of thing: the shape of somebody's window, which they arrange
//! once and expect to find again in every project they open.
//!
//! Both are read from and written to the same file through [`crate::services::store`]. Names in the file
//! are grouped with dots so that the file reads like the dialog: `appearance.font.size` is the size on
//! the Appearance page under the Font heading.

use crate::services::store::{Store, Values};

/// The width the explorer starts at, and the smallest and largest it can be dragged to.
pub const EXPLORER_WIDTH: f32 = 248.0;
pub const EXPLORER_MIN: f32 = 150.0;

/// What a pane a plugin contributed is, until its manifest says otherwise.
///
/// The explorer's width and the terminal's height, because those are the two numbers the window already
/// uses for a column at the side and a strip along the bottom, and a plugin that names neither should
/// look like it belongs rather than like it guessed.
pub const PLUGIN_PANE_WIDTH: f32 = 320.0;
pub const PLUGIN_PANE_HEIGHT: f32 = 260.0;
/// The narrowest and shortest a contributed pane is ever dragged to.
pub const PLUGIN_PANE_MIN_WIDTH: f32 = 220.0;
pub const PLUGIN_PANE_MIN_HEIGHT: f32 = 120.0;
pub const EXPLORER_MAX: f32 = 620.0;

/// The height the terminal starts at, and its limits.
pub const TERMINAL_HEIGHT: f32 = 260.0;
pub const TERMINAL_MIN: f32 = 90.0;

/// The same for the run tile, which is the terminal tile's sibling and sits in the same place.
///
/// Its own entry rather than a shared one, because the two hold different things: a terminal is
/// typed into and a run is read, and somebody who wants a tall log and a short shell should get
/// both. They start at the same height because they are the same shape.
pub const RUN_HEIGHT: f32 = TERMINAL_HEIGHT;
pub const RUN_MIN: f32 = TERMINAL_MIN;

/// And the same again for the debug tile, the third of the three, for the same reason: a stack and a
/// tree of variables are read at a different height from a log, and somebody who wants a tall one
/// and a short shell should get both.
pub const DEBUG_HEIGHT: f32 = 300.0;
pub const DEBUG_MIN: f32 = 120.0;

/// The other measurement each panel needs, now that a panel can be docked to any edge — `task-1697`.
///
/// A panel at the side is a column and is read by its **width**; a panel in a strip along the top or
/// the bottom is read by its **height**. One number cannot be both: the terminal is 260 points tall
/// along the bottom, and a 260 point wide column down the right is half a terminal. So each panel
/// carries two, four of which already existed and four of which are these.
///
/// The explorer's height is a tile's height, because that is what it is when it is in a strip. The
/// three tiles' widths are a comfortable terminal column — 420 points is about eighty columns of the
/// monospaced face at its default size — and the debug tile's is wider because it holds two panes
/// side by side and `debug_panel::PANE_MIN` says how narrow each of those can get.
pub const EXPLORER_HEIGHT: f32 = TERMINAL_HEIGHT;
pub const EXPLORER_HEIGHT_MIN: f32 = TERMINAL_MIN;
pub const TERMINAL_WIDTH: f32 = 420.0;
pub const TERMINAL_WIDTH_MIN: f32 = 220.0;
pub const RUN_WIDTH: f32 = TERMINAL_WIDTH;
pub const RUN_WIDTH_MIN: f32 = TERMINAL_WIDTH_MIN;
pub const DEBUG_WIDTH: f32 = 520.0;
pub const DEBUG_WIDTH_MIN: f32 = 300.0;

/// The widest a panel read out of the settings file is believed.
///
/// **Not a limit on dragging any more.** It was 900 points and it was one, and `task-1771` reported the
/// consequence in the one place it bites hardest: with the editing area hidden, the Agent-Chat pane
/// stopped widening part way across a window that had nothing else in it. A number chosen here cannot
/// know how wide somebody's display is or what they have put away, and the layout already refuses to
/// give a panel more than there is — [`Panes::resize`] clamps against the room, and `dock::regions` shares
/// out what is left. So the drag is bounded by the window and by nothing else, which is what "as wide as I
/// want" means.
///
/// What is left is a sanity bound on a **hand edited or corrupted** settings file, so a width of ten
/// thousand does not have to be dragged back from. It is above any display this runs on, so nobody who
/// dragged a panel there can lose the size they chose.
///
/// The explorer keeps [`EXPLORER_MAX`], which is the number it has always had: it is a list of file names
/// rather than a document, its own tests pin that number, and nothing in the ticket asks for it to change.
pub const PANEL_MAX_WIDTH: f32 = 8192.0;

/// How opaque the background is when Unluminate starts. Not fully opaque, so the transparency is visible
/// without opening the settings. The design shows 83 per cent.
pub const DEFAULT_OPACITY: f32 = 0.83;
/// The lowest the opacity can be set to. Above zero, so the window cannot be lost entirely.
pub const MIN_OPACITY: f32 = 0.05;

/// The sizes the font size control offers.
pub const FONT_SIZES: &[f32] = &[9.0, 11.0, 13.0, 16.0, 20.0, 24.0, 32.0, 48.0, 64.0];
/// The size the editor sets text in until somebody chooses another, and what `Reset Font Size` goes
/// back to.
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
/// The smallest and largest the editor's font can be, whether it got there from the dialog, the
/// keyboard, a pinch or a hand edited settings file.
pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 144.0;

/// The sizes the terminal font size control offers.
pub const TERMINAL_FONT_SIZES: &[f32] = &[10.0, 11.0, 12.0, 13.0, 14.0, 16.0, 18.0, 20.0];

/// The point size egui sets a body of text in, which is what the window's own text has always been.
///
/// The default of `appearance.ui.font.size`, so an Unluminate that names no size in its settings file is drawn
/// exactly as it was before `task-1776` — `theme::apply_scaled` does nothing at all at a scale of one.
pub const DEFAULT_UI_FONT_SIZE: f32 = 12.5;
/// The sizes the interface font size control offers, which is the reference editor's own list around its default.
pub const UI_FONT_SIZES: &[f32] = &[10.0, 11.0, 12.5, 14.0, 16.0, 18.0, 20.0];
/// The smallest and largest the interface may be set to, whether from the dialog or a hand edited file.
///
/// Bounded harder than the editor's font, because this one moves the menus, the rail and the status bar:
/// a window whose furniture is set in six points cannot be used to put it back.
pub const MIN_UI_FONT_SIZE: f32 = 8.0;
pub const MAX_UI_FONT_SIZE: f32 = 24.0;

/// The next size up or down the list the Settings window offers.
///
/// The keyboard walks the same list the dialog does, so the two cannot come to disagree about what
/// sizes exist. A size that is not in the list — which a pinch produces, and which a hand edited
/// settings file may hold — steps to the nearest one past it in the direction asked for, so
/// pressing the key always moves and always lands somewhere the dialog can show.
pub fn step_font_size(from: f32, up: bool) -> f32 {
    let next = if up {
        FONT_SIZES.iter().copied().find(|size| *size > from + 0.01)
    } else {
        FONT_SIZES.iter().rev().copied().find(|size| *size < from - 0.01)
    };
    next.unwrap_or(if up { FONT_SIZES[FONT_SIZES.len() - 1] } else { FONT_SIZES[0] })
}

/// The zoom levels a pane that has no font size of its own steps through.
///
/// `task-1771` asks that **every** pane be zoomable, not only the editing area. Two of them already have
/// a point size a person chooses — the editor's font and the terminal's — and a zoom there walks that
/// setting, so there is one number saying how big the text is rather than a setting and a multiplier that
/// can disagree. The explorer and a pane a plugin contributed have no such setting, so their zoom is a
/// multiplier over everything they draw, and this is the ladder it walks.
///
/// It steps by about a sixth, which is the same feel as [`FONT_SIZES`] does around the default: 16 to 20
/// is a quarter, 13 to 16 is a fifth. Both ends are far enough out to be useless and are there so that
/// nobody finds a wheel that stops working — a fifth of normal is unreadable and three times it is a
/// heading, and one notch always moves.
pub const ZOOMS: &[f32] = &[0.5, 0.6, 0.7, 0.85, 1.0, 1.15, 1.35, 1.6, 1.9, 2.25, 2.65, 3.0];
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 3.0;
/// What a pane is zoomed to until somebody changes it, and what `Reset Font Size` puts it back to.
pub const DEFAULT_ZOOM: f32 = 1.0;

/// The next terminal font size up or down the list the Settings window offers for it.
///
/// Its own list, because the terminal's is not the editor's: [`TERMINAL_FONT_SIZES`] stops at twenty,
/// which is a very large character grid, where the editor goes to sixty-four. `task-1771` makes the wheel
/// over a terminal walk this, so the tile's zoom and the number in Settings are one thing.
pub fn step_terminal_font_size(from: f32, up: bool) -> f32 {
    let next = if up {
        TERMINAL_FONT_SIZES.iter().copied().find(|size| *size > from + 0.01)
    } else {
        TERMINAL_FONT_SIZES.iter().rev().copied().find(|size| *size < from - 0.01)
    };
    next.unwrap_or(if up {
        TERMINAL_FONT_SIZES[TERMINAL_FONT_SIZES.len() - 1]
    } else {
        TERMINAL_FONT_SIZES[0]
    })
}

/// The next zoom up or down [`ZOOMS`], by the rule [`step_font_size`] follows for the font sizes.
pub fn step_zoom(from: f32, up: bool) -> f32 {
    let next = if up {
        ZOOMS.iter().copied().find(|zoom| *zoom > from + 0.001)
    } else {
        ZOOMS.iter().rev().copied().find(|zoom| *zoom < from - 0.001)
    };
    next.unwrap_or(if up { MAX_ZOOM } else { MIN_ZOOM })
}

/// Whether the completion popup appears without being asked for.
///
/// Two values rather than three, and the reason is worth writing down: the person this setting
/// exists for is the one who found the popup arriving unasked distracting, and for them `manual` is
/// already the off switch — `Ctrl+Space`, the `Complete Word` entry and the command line all still
/// work, and nothing appears until one of them is asked. A third value meaning "off altogether"
/// would take away a key that never interrupts anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Suggestions {
    /// The popup opens while you type, at a two character stem. The reference editor's own default.
    #[default]
    Automatic,
    /// Nothing opens unless it is asked for.
    Manual,
}

impl Suggestions {
    /// The word the settings file, the command line and a test spell it with.
    pub fn name(self) -> &'static str {
        match self {
            Suggestions::Automatic => "automatic",
            Suggestions::Manual => "manual",
        }
    }

    /// Read a value, or nothing when the file holds something this version does not have — the same
    /// answer `plugin.kind` gives, for the same reason.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "automatic" => Some(Suggestions::Automatic),
            "manual" => Some(Suggestions::Manual),
            _ => None,
        }
    }

    /// True when the popup is allowed to open on its own, which is what the tick box shows.
    pub fn is_automatic(self) -> bool {
        self == Suggestions::Automatic
    }
}

/// Whether Unluminate asks the releases page for a newer version when it starts.
///
/// **Off, and that is the whole design rather than a cautious default.** `task-1692` drew the line
/// the chat pane keeps -- *"there is no discovery, no model list, no telemetry and nothing at
/// startup"* -- and an editor that phones home the moment it opens is exactly what that rule exists
/// to prevent. `Unluminate -> Check for Updates` is a person asking, and it works either way; this
/// is a person saying *ask every time I open it*, once. `task-1804` §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateCheck {
    /// Nothing is ever sent unless somebody asks.
    #[default]
    Off,
    /// Ask once, when the window opens.
    Start,
}

impl UpdateCheck {
    /// The word the settings file, the command line and a test spell it with.
    pub fn name(self) -> &'static str {
        match self {
            UpdateCheck::Off => "off",
            UpdateCheck::Start => "start",
        }
    }

    /// Read a value, or nothing when the file holds something this version does not have.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "off" | "never" => Some(UpdateCheck::Off),
            "start" | "startup" | "on" => Some(UpdateCheck::Start),
            _ => None,
        }
    }

    /// True when the window should ask as it opens.
    pub fn at_start(self) -> bool {
        self == UpdateCheck::Start
    }
}

/// What line breaks a file is written back with.
///
/// `task-1804` §7.1 made the line ending a value the document carries, read from the file when it is
/// opened. This is the person's say over it, and its default is to have no say at all: **a file is
/// written the way it was read**, which is the only answer that never produces a whole-file diff out
/// of a one character edit.
///
/// The other two exist for the project that has decided, where a file that arrived the other way
/// should be brought into line rather than kept as it is. A new file gets the platform's own either
/// way, because there is nothing to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEndings {
    /// Written the way it was read. What every editor worth using does.
    #[default]
    Keep,
    /// Always `\n`.
    Lf,
    /// Always `\r\n`.
    Crlf,
}

impl LineEndings {
    /// The word the settings file, the command line and a test spell it with.
    pub fn name(self) -> &'static str {
        match self {
            LineEndings::Keep => "keep",
            LineEndings::Lf => "lf",
            LineEndings::Crlf => "crlf",
        }
    }

    /// Read a value, or nothing when the file holds something this version does not have.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "keep" | "auto" => Some(LineEndings::Keep),
            "lf" | "unix" => Some(LineEndings::Lf),
            "crlf" | "windows" | "dos" => Some(LineEndings::Crlf),
            _ => None,
        }
    }

    /// What a file read as `found` should be written as.
    pub fn applied_to(self, found: unluminate_core::LineEnding) -> unluminate_core::LineEnding {
        match self {
            LineEndings::Keep => found,
            LineEndings::Lf => unluminate_core::LineEnding::Lf,
            LineEndings::Crlf => unluminate_core::LineEnding::Crlf,
        }
    }
}

/// Whether the debugger's value tooltip appears without being asked for.
///
/// The reference editor's `Show value tooltip`, in `Suggestions`' shape and for its reason: `manual` is already
/// the off switch, because `Debug -> Show Value` and `unluminate-cli debug hover` work either way, so a
/// third value meaning "off altogether" would take away a control that never interrupts anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueTooltip {
    /// The popup arrives when the pointer has rested on a name for `HOVER_DELAY`. The reference editor's default.
    #[default]
    Automatic,
    /// Nothing appears unless it is asked for.
    Manual,
}

impl ValueTooltip {
    /// The word the settings file, the command line and a test spell it with.
    pub fn name(self) -> &'static str {
        match self {
            ValueTooltip::Automatic => "automatic",
            ValueTooltip::Manual => "manual",
        }
    }

    /// Read a value, or nothing when the file holds something this version does not have.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "automatic" => Some(ValueTooltip::Automatic),
            "manual" => Some(ValueTooltip::Manual),
            _ => None,
        }
    }

    /// True when the popup is allowed to arrive on its own, which is what the tick box shows.
    pub fn is_automatic(self) -> bool {
        self == ValueTooltip::Automatic
    }
}

/// One page of the Settings window, and the group it is listed under.
///
/// The list on the left of the window is built from this, so adding a page is one variant and one match
/// arm rather than a change to the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// The editor's font and the window's background.
    #[default]
    Appearance,
    /// The theme, the accent and the icon set: what every colour in the window means.
    ///
    /// A page of its own rather than a section on Appearance, because the Settings window is one size for
    /// every page, no page scrolls, and Appearance already fills its 640 points. The reference editor separates them
    /// too — Appearance holds the theme and Editor holds the colour scheme — and here the two are one
    /// question, because an Unluminate theme carries both.
    Theme,
    /// The editing area itself: the gutter, and the colour scheme code is set in.
    Editor,
    /// The plugins that are installed, and the marketplace they came from.
    Plugins,
    /// The terminal at the bottom of the window.
    Terminal,
    /// The Model Context Protocol server, which is how an AI agent drives Unluminate.
    Mcp,
    /// A page a plugin contributed, by its place in `plugins::Surfaces::pages`.
    ///
    /// A slot rather than a name, so `Page` stays `Copy` — it is passed by value through the dialog and
    /// through `MenuState`. Which page a slot is comes from the manifests, and the page's own name comes
    /// with it, so [`Page::label`] answers `Plugin` with a placeholder and the dialog asks the surfaces
    /// for the real one. Nothing persists a chosen page, so a slot shifting when a plugin is switched off
    /// costs nothing.
    Plugin(u8),
}

impl Page {
    /// Unluminate's own six. A plugin's page is not in it, because there is no compile time list of them;
    /// [`Page::all`] is the one that includes them.
    pub const ALL: [Page; 6] =
        [Page::Appearance, Page::Theme, Page::Editor, Page::Plugins, Page::Terminal, Page::Mcp];

    /// Unluminate's own six, then one per plugin that contributed a page, in the order the plugins are
    /// listed.
    pub fn all(contributed: usize) -> Vec<Page> {
        let mut found = Page::ALL.to_vec();
        found.extend((0..contributed).map(|slot| Page::Plugin(slot as u8)));
        found
    }

    /// The slot number, for a page a plugin contributed.
    pub fn plugin_slot(self) -> Option<usize> {
        match self {
            Page::Plugin(slot) => Some(slot as usize),
            _ => None,
        }
    }

    /// The name in the list on the left, and the last part of the heading.
    pub fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Theme => "Theme",
            Page::Editor => "Editor",
            Page::Plugins => "Plugins",
            Page::Terminal => "Terminal",
            Page::Mcp => "MCP",
            // A contributed page's real name is `settings.page` in its manifest, which this function has
            // no way to reach. `settings_dialog::title_of` is what draws it, asking the surfaces; this is
            // the answer for a slot with no plugin in it, which nothing draws.
            Page::Plugin(_) => "Plugin",
        }
    }

    /// The heading the page is listed under, which is also the first part of the breadcrumb.
    pub fn group(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance & Behavior",
            // Beside Appearance, which is where the reference editor keeps the theme picker.
            Page::Theme => "Appearance & Behavior",
            Page::Editor => "Editor",
            // No heading of its own, the way the reference editor lists Plugins: it is one page rather than a
            // group with pages under it.
            Page::Plugins => "",
            Page::Terminal => "Tools",
            // The same heading as the terminal: both are a way of reaching something outside the
            // editor from inside it.
            Page::Mcp => "Tools",
            // A contributed page is listed under `Plugins`, so a person looking for what a plugin added
            // finds it under the heading that says where it came from.
            Page::Plugin(_) => "Plugins",
        }
    }

    /// The headings inside the page. They are what the search box matches on as well as being drawn.
    pub fn sections(self) -> &'static [&'static str] {
        match self {
            Page::Appearance => &["Font", "Interface", "Background"],
            Page::Theme => &["Theme", "Accent", "Icons"],
            Page::Editor => &["Gutter", "Suggestions", "Debugger"],
            Page::Plugins => &["Marketplace", "Installed", "Colour Scheme", "Syntax"],
            Page::Terminal => &["Font", "Shell"],
            Page::Mcp => &["Install", "Server", "Configuration"],
            // A contributed page's headings are its own, and a manifest does not name them: the plugin
            // draws its page. So it has none here, which means the search box finds it by its name.
            Page::Plugin(_) => &[],
        }
    }

    /// True when this page is worth showing for what has been typed in the search box.
    pub fn matches(self, search: &str) -> bool {
        let needle = search.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        let haystacks = [self.title(), self.group()];
        haystacks.iter().any(|text| text.to_lowercase().contains(&needle))
            || self.sections().iter().any(|text| text.to_lowercase().contains(&needle))
    }
}

/// The settings a person chooses.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The family the editor sets text in.
    pub font_family: String,
    /// The point size the editor sets text in.
    pub font_size: f32,
    /// How opaque the window background is, which is what lets the desktop show through.
    pub opacity: f32,
    /// The point size the terminal sets its grid in.
    pub terminal_font_size: f32,
    /// The program a new terminal runs. Empty means the one this machine says the person has, which
    /// `unluminate_terminal::session` decides and which is PowerShell on Windows.
    pub terminal_shell: String,
    /// Whether the editing area has a column of line numbers down its left.
    pub line_numbers: bool,
    /// Whether the completion popup arrives while you type, or waits to be asked.
    pub suggestions: Suggestions,
    /// What line breaks a file is written back with. See [`LineEndings`].
    pub line_endings: LineEndings,
    /// Whether the window asks for a newer version as it opens. See [`UpdateCheck`].
    pub update_check: UpdateCheck,
    /// Extra patterns the project index leaves out, beyond `.gitignore` and the build folders.
    ///
    /// `task-1804` §7.3: the index skipped three hardcoded folder names and read no ignore file at
    /// all, so `editor definition` answered out of a gitignored scratch copy of the whole project.
    /// `.gitignore` is now read; this is the person's own list beside it, for the folder that is not
    /// a repository and for the pattern a repository has a reason not to ignore.
    ///
    /// Written as one line of comma separated patterns, in `.gitignore`'s own syntax, because that
    /// is the syntax a person already knows and the one the reader beside it implements.
    pub exclude: String,
    /// Whether the debugger's value tooltip arrives when the pointer rests on a name.
    pub value_tooltip: ValueTooltip,
    /// Whether a plugin that asked for the decoration renderer gets it.
    ///
    /// The soft shadows, inset shadows and gradients `services::vello_canvas` draws behind a plugin's pane.
    /// Off, the board draws flat, which is what it did before `task-1765`. It is a setting for the reason
    /// the opacity is one: it is the person's window, and drawing depth costs a rasterisation on the frame
    /// a board changes.
    pub plugin_chrome: bool,
    /// Whether this Unluminate hosts the MCP server over HTTP, so an agent can reach it at a URL.
    pub mcp_enabled: bool,
    /// The port it listens on when it does.
    pub mcp_port: u16,
    /// How many tools the catalogue is cut into for an agent. See `unluminate_cli::mcp::tools`.
    pub mcp_tools: unluminate_cli::mcp::Shape,
    /// Which areas of the catalogue the hosted server offers, separated by commas.
    ///
    /// Empty means all of them, which is what it always did. `task-1804` §4.2 measured what all
    /// of them costs -- 18% of a local model's window before a question is asked -- and this is the
    /// same lever `mcp serve --areas` gives the server an agent launches itself.
    pub mcp_areas: String,
    /// Which theme the window is painted in, as `<plugin>/<theme>` — `themes-bundle-1/dracula`.
    ///
    /// Empty means `unluminate/dark`, which is the palette Unluminate shipped with. Empty rather than the key
    /// written out, for `terminal_shell`'s reason once more: a settings file that names nothing is a file
    /// that asks for whatever this Unluminate's default is, and one that names a theme whose plugin has been
    /// switched off falls back to it rather than to nothing.
    pub theme: String,
    /// One colour used for everything the accent means, over whatever the theme said.
    ///
    /// Written as `#RRGGBB`, empty meaning the theme's own. Material Theme UI's best known setting.
    pub accent: String,
    /// Which drawn icon set is used, from `plugins::ICON_SETS`. Empty means the theme's own choice,
    /// which is what `Follow the theme` is on the Theme page.
    pub icons: String,
    /// The family the window's own text is set in — menus, the explorer, the status bar.
    ///
    /// Empty means the editor's family, which is what the interface was always set in and is why this can
    /// be added without anything moving. The reference editor's `Appearance -> Use custom font`.
    pub ui_font_family: String,
    /// The point size the window's own text is set in. [`DEFAULT_UI_FONT_SIZE`] leaves egui exactly as it
    /// was.
    pub ui_font_size: f32,
    /// Where each debug adapter lives, when this machine keeps one somewhere Unluminate would not look.
    ///
    /// `debug.lldb`, `debug.node` — one entry per name in `plugins::DEBUGGERS`, and empty meaning
    /// "whatever this machine has", which is `Settings::shell()`'s sentence made once more. It is a
    /// path rather than a preference, so a settings file copied to another machine names nothing.
    pub debug_adapters: Vec<(String, String)>,
}

impl Settings {
    /// The settings an Unluminate that has never been run has. The family is decided by the renderer, because
    /// it depends on what the system has installed, so it is left empty here and filled in by the window.
    pub fn new() -> Self {
        Self {
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            opacity: DEFAULT_OPACITY,
            terminal_font_size: 13.0,
            // Empty rather than a name, because which shell is right is a question about the machine
            // Unluminate is running on, and the settings file is copied between machines.
            terminal_shell: String::new(),
            // On, because a line number is useful in prose as well as in code and a person who does
            // not want one can put it away from the gutter's own menu.
            line_numbers: true,
            // On, which is what the reference editor's own "Show suggestions as you type" is, and what the
            // ticket asked for: suggestions that arrive rather than ones you have to remember to
            // ask for.
            suggestions: Suggestions::Automatic,
            // Keep, and the type's own comment argues it: any other default rewrites somebody's file
            // the first time they type in it.
            line_endings: LineEndings::Keep,
            // Off. See the type's own comment: nothing is sent unless somebody asks.
            update_check: UpdateCheck::Off,
            // Empty. `.gitignore` is read whether or not this names anything, and a pattern here is
            // an addition to it rather than a replacement for it.
            exclude: String::new(),
            // On, which is what the reference editor's own `Show value tooltip` is: the whole point of the
            // feature is that the value is there when you look at the name, rather than being
            // something to remember to ask for.
            value_tooltip: ValueTooltip::Automatic,
            // On. A board that draws flat is the board `task-1765` was filed about.
            plugin_chrome: true,
            // Off, and the reason is worth writing down rather than being read off as timidity.
            // The MCP server an agent launches over its own pipes needs no port and no setting: it
            // lives as long as the conversation and nothing is listening when nobody is asking,
            // which is what `Settings -> Tools -> MCP`'s install buttons set up. A fixed open port
            // that will run `terminal send` for anything that can reach it is a different
            // proposition, and it should be a thing somebody turned on rather than a thing they
            // were given.
            mcp_enabled: false,
            mcp_port: unluminate_cli::mcp::DEFAULT_PORT,
            mcp_tools: unluminate_cli::mcp::Shape::default(),
            // Empty: every area, which is what a configuration that names none has always meant.
            mcp_areas: String::new(),
            // The four `task-1776` added, each empty or at the number that changes nothing, so an Unluminate
            // that has never been run is painted in exactly the palette it always was.
            theme: String::new(),
            accent: String::new(),
            icons: String::new(),
            ui_font_family: String::new(),
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            // Empty, for `terminal_shell`'s reason: where `lldb-dap` lives is a question about this
            // machine, and the settings file is copied between machines.
            debug_adapters: Vec::new(),
        }
    }

    pub fn read_from(values: &Values) -> Self {
        let mut settings = Self::new();
        if let Some(family) = values.text("appearance.font.family") {
            settings.font_family = family.to_owned();
        }
        if let Some(size) = values.number("appearance.font.size") {
            settings.font_size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        }
        if let Some(opacity) = values.number("appearance.background.opacity") {
            settings.opacity = opacity.clamp(MIN_OPACITY, 1.0);
        }
        if let Some(size) = values.number("terminal.font.size") {
            settings.terminal_font_size = size.clamp(6.0, 48.0);
        }
        if let Some(shell) = values.text("terminal.shell") {
            settings.terminal_shell = shell.trim().to_owned();
        }
        if let Some(on) = values.flag("editor.line_numbers") {
            settings.line_numbers = on;
        }
        if let Some(chosen) = values.text("debug.value_tooltip").and_then(ValueTooltip::parse) {
            settings.value_tooltip = chosen;
        }
        if let Some(chosen) = values.text("editor.suggestions").and_then(Suggestions::parse) {
            settings.suggestions = chosen;
        }
        if let Some(chosen) = values.text("editor.line_ending").and_then(LineEndings::parse) {
            settings.line_endings = chosen;
        }
        if let Some(chosen) = values.text("update.check").and_then(UpdateCheck::parse) {
            settings.update_check = chosen;
        }
        if let Some(patterns) = values.text("editor.exclude") {
            settings.exclude = patterns.trim().to_owned();
        }
        if let Some(on) = values.flag("plugins.chrome") {
            settings.plugin_chrome = on;
        }
        if let Some(on) = values.flag("mcp.enabled") {
            settings.mcp_enabled = on;
        }
        if let Some(port) = values.number("mcp.port") {
            settings.mcp_port = clamp_port(port);
        }
        if let Some(shape) =
            values.text("mcp.tools").and_then(unluminate_cli::mcp::Shape::parse)
        {
            settings.mcp_tools = shape;
        }
        if let Some(areas) = values.text("mcp.areas") {
            settings.mcp_areas = areas.trim().to_owned();
        }
        if let Some(theme) = values.text("appearance.theme") {
            settings.theme = theme.trim().to_owned();
        }
        if let Some(accent) = values.text("appearance.accent") {
            settings.accent = accent.trim().to_owned();
        }
        if let Some(icons) = values.text("appearance.icons") {
            settings.icons = icons.trim().to_lowercase();
        }
        if let Some(family) = values.text("appearance.ui.font.family") {
            settings.ui_font_family = family.trim().to_owned();
        }
        if let Some(size) = values.number("appearance.ui.font.size") {
            settings.ui_font_size = size.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE);
        }
        for name in crate::services::plugins::DEBUGGERS {
            if let Some(path) =
                values.text(&format!("debug.{name}")).map(str::trim).filter(|path| !path.is_empty())
            {
                settings.debug_adapters.push(((*name).to_owned(), path.to_owned()));
            }
        }
        settings
    }

    pub fn write_into(&self, values: &mut Values) {
        // **A setting that has gone back to its default is taken out of the file, not left in it.**
        // Seven of these mean "whatever this Unluminate's own default is" by having *no line at all* — a
        // blank `terminal.shell` would read as a shell called nothing, and a blank `appearance.theme`
        // as a theme called nothing — and `settings::save_with` merges over the file that is already
        // there. Written with an `if !is_empty` alone, a value that was cleared stayed in the file and
        // came back at the next start: choosing Monokai Pro and then choosing Unluminate Dark again would
        // have come up in Monokai Pro tomorrow, and `Follow the theme` and `The theme's own` are
        // first-class choices on the Theme page rather than corners. Found by driving a real window.
        values.set_or_clear("appearance.font.family", &self.font_family);
        values.set("appearance.font.size", format!("{:.0}", self.font_size));
        values.set("appearance.background.opacity", format!("{:.3}", self.opacity));
        values.set("terminal.font.size", format!("{:.0}", self.terminal_font_size));
        values.set_or_clear("terminal.shell", &self.terminal_shell);
        values.set_or_clear("appearance.theme", &self.theme);
        values.set_or_clear("appearance.accent", &self.accent);
        values.set_or_clear("appearance.icons", &self.icons);
        values.set_or_clear("appearance.ui.font.family", &self.ui_font_family);
        values.set("appearance.ui.font.size", format!("{:.1}", self.ui_font_size));
        values.set("editor.line_numbers", if self.line_numbers { "true" } else { "false" });
        values.set("editor.suggestions", self.suggestions.name());
        values.set("editor.line_ending", self.line_endings.name());
        values.set("update.check", self.update_check.name());
        values.set_or_clear("editor.exclude", &self.exclude);
        values.set("debug.value_tooltip", self.value_tooltip.name());
        values.set("plugins.chrome", if self.plugin_chrome { "true" } else { "false" });
        values.set("mcp.enabled", if self.mcp_enabled { "true" } else { "false" });
        values.set("mcp.port", self.mcp_port.to_string());
        values.set("mcp.tools", self.mcp_tools.name());
        values.set_or_clear("mcp.areas", &self.mcp_areas);
        // Written only once one has been chosen, exactly as the shell is, so a settings file does
        // not name a path that exists on one machine and not on the next — and taken out again when it
        // is cleared, for the reason at the top of this function. The list is walked rather than the
        // vec, because an adapter that was cleared is not in the vec at all.
        for name in crate::services::plugins::DEBUGGERS {
            values.set_or_clear(&format!("debug.{name}"), self.debug_adapter(name).unwrap_or_default());
        }
    }

    /// Which areas the hosted MCP server offers, read into the type that does the filtering.
    ///
    /// A name the catalogue has not got is **dropped here** rather than refusing the whole line,
    /// which is the opposite of what `mcp serve --areas` does with the same text -- and deliberately
    /// so: a command line is a person typing now and can be told; a settings file is read at
    /// startup, may have been written by an older or newer Unluminate, and the rule this file already
    /// keeps for a value it does not recognise is to fall back rather than refuse to start.
    pub fn mcp_area_filter(&self) -> unluminate_cli::mcp::tools::Areas {
        unluminate_cli::mcp::tools::Areas::parse(&self.mcp_areas).unwrap_or_else(|_| {
            let kept: Vec<&str> = self
                .mcp_areas
                .split(',')
                .map(str::trim)
                .filter(|name| unluminate_cli::catalogue::areas().iter().any(|area| area.eq_ignore_ascii_case(name)))
                .collect();
            unluminate_cli::mcp::tools::Areas::parse(&kept.join(",")).unwrap_or_default()
        })
    }

    /// The path this machine's settings give for the debug adapter called `name`, or nothing when
    /// they name none and Unluminate should look for it itself.
    ///
    /// [`Settings::shell`]'s sentence, made once more and in one function rather than the same
    /// `is_empty` test wherever an adapter is started.
    pub fn debug_adapter(&self, name: &str) -> Option<&str> {
        self.debug_adapters
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, path)| path.as_str())
    }

    /// How much larger or smaller than egui's own the window's text is set.
    ///
    /// One function rather than the division written wherever the interface is laid out, so the size in
    /// the dialog and the scale egui is given can never come to disagree — the rule [`Settings::shell`]
    /// keeps for the shell.
    pub fn interface_scale(&self) -> f32 {
        (self.ui_font_size / DEFAULT_UI_FONT_SIZE).clamp(0.6, 2.0)
    }

    /// The accent this settings file asks for, or nothing when it asks for the theme's own.
    pub fn accent_colour(&self) -> Option<egui::Color32> {
        let read = crate::services::plugins::colour(self.accent.trim())?;
        Some(egui::Color32::from_rgb(read.r, read.g, read.b))
    }

    /// Which drawn icon set to use, or nothing when the theme's own choice is wanted.
    ///
    /// A name Unluminate has not got reads as "follow the theme" rather than as an error, which is the rule
    /// `store.rs` keeps for a settings file with a stray line in it: a hand edited value should never be
    /// the reason a window comes up with no icons.
    pub fn icon_set(&self) -> Option<crate::theme::IconSet> {
        crate::theme::IconSet::parse(&self.icons)
    }

    /// The program a new terminal should run, or nothing when this machine's own default is wanted.
    ///
    /// One function rather than the same `is_empty` test at each of the places that start a terminal,
    /// so a later one cannot come to a different answer about what an empty setting means.
    pub fn shell(&self) -> Option<String> {
        let shell = self.terminal_shell.trim();
        (!shell.is_empty()).then(|| shell.to_owned())
    }

    /// The change to hand to `Document::set_base_style` so the document is shown in this font.
    pub fn as_style_change(&self) -> unluminate_core::StyleChange {
        unluminate_core::StyleChange {
            family: (!self.font_family.is_empty()).then(|| self.font_family.clone()),
            size: Some(self.font_size),
            ..unluminate_core::StyleChange::default()
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

/// A port brought inside its limits.
///
/// Every setting here is clamped rather than refused, which is what a slider does and what a hand
/// edited file gets. A port below 1024 needs privileges on macOS and is never what anybody meant,
/// and there is nothing above 65535 to be had.
pub fn clamp_port(port: f32) -> u16 {
    port.clamp(unluminate_cli::mcp::MIN_PORT as f32, u16::MAX as f32) as u16
}

/// Where the draggable dividers were left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panes {
    /// How wide the explorer is.
    pub explorer_width: f32,
    /// How tall the explorer is when it is in a strip along the top or the bottom — `task-1697`.
    pub explorer_height: f32,
    /// How tall the terminal tile is.
    pub terminal_height: f32,
    /// How wide the terminal tile is when it is a column at the side — `task-1697`.
    pub terminal_width: f32,
    /// How tall the run tile is. Its own measurement — see [`RUN_HEIGHT`].
    pub run_height: f32,
    /// How wide the run tile is as a column.
    pub run_width: f32,
    /// How tall the debug tile is. Its own too — see [`DEBUG_HEIGHT`].
    pub debug_height: f32,
    /// How wide the debug tile is as a column.
    pub debug_width: f32,
    /// How much bigger or smaller the explorer draws everything in it than it does by default.
    ///
    /// `task-1771`: every pane is zoomable with `Ctrl`/`Cmd` and the wheel. The explorer has no font size
    /// of its own — its rows, its indents and its lettering are the style guide's numbers — so its zoom is
    /// a multiplier over all of them rather than a second point size. See [`ZOOMS`].
    pub explorer_zoom: f32,
    /// The same for each pane a plugin contributed, by slot. It reaches the pane through `Look::scale`, so
    /// a provider that already scales with the editor's font gets this for nothing.
    pub plugin_zooms: [f32; crate::app::dock::PLUGIN_PANES],
    /// How wide and how tall each pane a plugin contributed is, by slot.
    ///
    /// Two arrays for the same reason the four panels have two numbers each: one number cannot be both a
    /// column's width and a strip's height. They start at what the manifest asked for, and a drag on the
    /// pane's divider changes them like any other panel's.
    pub plugin_widths: [f32; crate::app::dock::PLUGIN_PANES],
    pub plugin_heights: [f32; crate::app::dock::PLUGIN_PANES],
    /// Which edge each panel is docked to, and where in that edge — `task-1697`.
    ///
    /// It lives here, in the person's own settings, rather than in the project's `.unluminate`: the
    /// window's *geometry* belongs to the project, because Unluminate's windows are one per project and a
    /// geometry kept per person would open the second window on top of the first — but the window's
    /// *shape* is a habit, and somebody who works with the terminal on the right wants it on the
    /// right in every project.
    pub dock: crate::app::dock::Layout,
    /// How much of the editing area the source takes in the side by side view, from 0.15 to 0.85.
    pub preview_fraction: f32,
    /// How much of the `Find in Files` modal the results take, the rest going to the preview of the
    /// file under them. A pane inside a modal is still a pane, so where its divider was left is
    /// remembered like every other one.
    pub find_split: f32,
    /// The same for the references, candidates and rename modal, which has the same two panes and
    /// is very often wanted at a different size: a change set is read as a list, and a reference is
    /// read in the file round it.
    pub references_split: f32,
}

impl Panes {
    pub fn new() -> Self {
        Self {
            explorer_width: EXPLORER_WIDTH,
            explorer_height: EXPLORER_HEIGHT,
            terminal_height: TERMINAL_HEIGHT,
            terminal_width: TERMINAL_WIDTH,
            run_height: RUN_HEIGHT,
            run_width: RUN_WIDTH,
            debug_height: DEBUG_HEIGHT,
            debug_width: DEBUG_WIDTH,
            explorer_zoom: DEFAULT_ZOOM,
            plugin_zooms: [DEFAULT_ZOOM; crate::app::dock::PLUGIN_PANES],
            plugin_widths: [PLUGIN_PANE_WIDTH; crate::app::dock::PLUGIN_PANES],
            plugin_heights: [PLUGIN_PANE_HEIGHT; crate::app::dock::PLUGIN_PANES],
            dock: crate::app::dock::Layout::new(),
            preview_fraction: 0.5,
            find_split: crate::components::find_in_files::SPLIT,
            references_split: crate::components::references::SPLIT,
        }
    }

    pub fn read_from(values: &Values) -> Self {
        Self::read_from_with(values, &[])
    }

    /// The same, told the names of the panes plugins contributed, in slot order.
    ///
    /// A contributed pane's two measurements are recorded against its own `<plugin id>/<pane id>` rather
    /// than against its slot number, for the reason its side is: installing a second plugin must not give
    /// the first one's pane the second one's width.
    pub fn read_from_with(values: &Values, plugin_panes: &[String]) -> Self {
        let mut panes = Self::read_built_in(values);
        for (slot, key) in plugin_panes.iter().enumerate().take(crate::app::dock::PLUGIN_PANES) {
            if let Some(width) = values.number(&format!("panes.{key}.width")) {
                panes.plugin_widths[slot] = width.clamp(PLUGIN_PANE_MIN_WIDTH, PANEL_MAX_WIDTH);
            }
            if let Some(height) = values.number(&format!("panes.{key}.height")) {
                panes.plugin_heights[slot] = height.max(PLUGIN_PANE_MIN_HEIGHT);
            }
            if let Some(zoom) = values.number(&format!("panes.{key}.zoom")) {
                panes.plugin_zooms[slot] = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
            }
        }
        panes.dock = crate::app::dock::Layout::read_from_with(values, plugin_panes);
        panes
    }

    fn read_built_in(values: &Values) -> Self {
        let mut panes = Self::new();
        if let Some(width) = values.number("panes.explorer.width") {
            panes.explorer_width = width.clamp(EXPLORER_MIN, EXPLORER_MAX);
        }
        if let Some(zoom) = values.number("panes.explorer.zoom") {
            panes.explorer_zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        }
        if let Some(height) = values.number("panes.terminal.height") {
            panes.terminal_height = height.max(TERMINAL_MIN);
        }
        if let Some(height) = values.number("panes.run.height") {
            panes.run_height = height.max(RUN_MIN);
        }
        if let Some(height) = values.number("panes.debug.height") {
            panes.debug_height = height.max(DEBUG_MIN);
        }
        if let Some(height) = values.number("panes.explorer.height") {
            panes.explorer_height = height.max(EXPLORER_HEIGHT_MIN);
        }
        if let Some(width) = values.number("panes.terminal.width") {
            panes.terminal_width = width.clamp(TERMINAL_WIDTH_MIN, PANEL_MAX_WIDTH);
        }
        if let Some(width) = values.number("panes.run.width") {
            panes.run_width = width.clamp(RUN_WIDTH_MIN, PANEL_MAX_WIDTH);
        }
        if let Some(width) = values.number("panes.debug.width") {
            panes.debug_width = width.clamp(DEBUG_WIDTH_MIN, PANEL_MAX_WIDTH);
        }
        panes.dock = crate::app::dock::Layout::read_from(values);
        if let Some(fraction) = values.number("panes.preview.fraction") {
            panes.preview_fraction = fraction.clamp(0.15, 0.85);
        }
        if let Some(fraction) = values.number("panes.find.split") {
            panes.find_split = fraction.clamp(
                crate::components::find_in_files::SPLIT_MIN,
                crate::components::find_in_files::SPLIT_MAX,
            );
        }
        if let Some(fraction) = values.number("panes.references.split") {
            panes.references_split = fraction.clamp(
                crate::components::references::SPLIT_MIN,
                crate::components::references::SPLIT_MAX,
            );
        }
        panes
    }

    pub fn write_into(&self, values: &mut Values) {
        self.write_into_with(values, &[]);
    }

    /// The same, told the names of the panes plugins contributed, in slot order.
    pub fn write_into_with(&self, values: &mut Values, plugin_panes: &[String]) {
        self.write_built_in(values);
        for (slot, key) in plugin_panes.iter().enumerate().take(crate::app::dock::PLUGIN_PANES) {
            values.set(&format!("panes.{key}.zoom"), format!("{:.2}", self.plugin_zooms[slot]));
            values.set(&format!("panes.{key}.width"), format!("{:.0}", self.plugin_widths[slot]));
            values.set(&format!("panes.{key}.height"), format!("{:.0}", self.plugin_heights[slot]));
        }
        self.dock.write_into_with(values, plugin_panes);
    }

    fn write_built_in(&self, values: &mut Values) {
        values.set("panes.explorer.width", format!("{:.0}", self.explorer_width));
        values.set("panes.explorer.zoom", format!("{:.2}", self.explorer_zoom));
        values.set("panes.terminal.height", format!("{:.0}", self.terminal_height));
        values.set("panes.run.height", format!("{:.0}", self.run_height));
        values.set("panes.debug.height", format!("{:.0}", self.debug_height));
        values.set("panes.explorer.height", format!("{:.0}", self.explorer_height));
        values.set("panes.terminal.width", format!("{:.0}", self.terminal_width));
        values.set("panes.run.width", format!("{:.0}", self.run_width));
        values.set("panes.debug.width", format!("{:.0}", self.debug_width));
        values.set("panes.preview.fraction", format!("{:.3}", self.preview_fraction));
        values.set("panes.find.split", format!("{:.3}", self.find_split));
        values.set("panes.references.split", format!("{:.3}", self.references_split));
    }

    /// How wide `panel` asks to be when it is a column at the side of the window.
    ///
    /// The eight numbers above are four pairs, and these functions are the only place anything
    /// outside this file has to know which of a pair to read — the side decides, and `app::dock` is
    /// what asks. A fifth panel would be a fifth arm here and nowhere else.
    pub fn width_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => self.explorer_width,
            Panel::Terminal => self.terminal_width,
            Panel::Run => self.run_width,
            Panel::Debug => self.debug_width,
            Panel::Plugin(slot) => self.plugin_widths[(slot as usize).min(self.plugin_widths.len() - 1)],
        }
    }

    /// How tall `panel` asks to be when it is in a strip along the top or the bottom.
    pub fn height_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => self.explorer_height,
            Panel::Terminal => self.terminal_height,
            Panel::Run => self.run_height,
            Panel::Debug => self.debug_height,
            Panel::Plugin(slot) => self.plugin_heights[(slot as usize).min(self.plugin_heights.len() - 1)],
        }
    }

    /// Set a panel's width outright, which is what a divider **inside a strip** moves: there the
    /// panels share one depth and it is their widths that a divider between two of them changes.
    pub fn set_width_of(&mut self, panel: crate::app::dock::Panel, width: f32) {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => self.explorer_width = width,
            Panel::Terminal => self.terminal_width = width,
            Panel::Run => self.run_width = width,
            Panel::Debug => self.debug_width = width,
            Panel::Plugin(slot) => {
                let at = (slot as usize).min(self.plugin_widths.len() - 1);
                self.plugin_widths[at] = width;
            }
        }
    }

    /// The same for a panel's height.
    pub fn set_height_of(&mut self, panel: crate::app::dock::Panel, height: f32) {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => self.explorer_height = height,
            Panel::Terminal => self.terminal_height = height,
            Panel::Run => self.run_height = height,
            Panel::Debug => self.debug_height = height,
            Panel::Plugin(slot) => {
                let at = (slot as usize).min(self.plugin_heights.len() - 1);
                self.plugin_heights[at] = height;
            }
        }
    }

    /// How much bigger or smaller this panel draws everything in it.
    ///
    /// **One for the explorer and one a plugin pane, and 1.0 for the three tiles**, because a tile's zoom is
    /// the terminal's own font size — the one number the Settings window shows for it — and a multiplier on
    /// top of that would be a second way of saying the same thing. See [`ZOOMS`] and
    /// `UnluminateApp::zoom_the_panel`.
    pub fn zoom_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => self.explorer_zoom,
            Panel::Plugin(slot) => self.plugin_zooms[(slot as usize).min(self.plugin_zooms.len() - 1)],
            _ => DEFAULT_ZOOM,
        }
    }

    /// Set it, clamped to the ladder's own ends. Does nothing for a panel whose zoom is a font size.
    pub fn set_zoom_of(&mut self, panel: crate::app::dock::Panel, zoom: f32) {
        use crate::app::dock::Panel;
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        match panel {
            Panel::Explorer => self.explorer_zoom = zoom,
            Panel::Plugin(slot) => {
                let at = (slot as usize).min(self.plugin_zooms.len() - 1);
                self.plugin_zooms[at] = zoom;
            }
            _ => {}
        }
    }

    pub fn min_width_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => EXPLORER_MIN,
            Panel::Terminal => TERMINAL_WIDTH_MIN,
            Panel::Run => RUN_WIDTH_MIN,
            Panel::Debug => DEBUG_WIDTH_MIN,
            Panel::Plugin(_) => PLUGIN_PANE_MIN_WIDTH,
        }
    }

    /// The widest this panel may be dragged, before the room there actually is is taken into account.
    ///
    /// Unbounded for everything but the explorer — see [`PANEL_MAX_WIDTH`]. `resize` takes the smaller of
    /// this and the room, so an unbounded panel is one bounded by the window, which is the honest limit.
    pub fn max_width_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => EXPLORER_MAX,
            _ => f32::INFINITY,
        }
    }

    pub fn min_height_of(&self, panel: crate::app::dock::Panel) -> f32 {
        use crate::app::dock::Panel;
        match panel {
            Panel::Explorer => EXPLORER_HEIGHT_MIN,
            Panel::Terminal => TERMINAL_MIN,
            Panel::Run => RUN_MIN,
            Panel::Debug => DEBUG_MIN,
            Panel::Plugin(_) => PLUGIN_PANE_MIN_HEIGHT,
        }
    }

    /// Set the measurement the side this panel is on actually reads, by `by` points.
    ///
    /// One function, so a divider, `panel size` and the settings file cannot come to three different
    /// answers about which of a pair a drag moved. `room` is how much there is to give, which is
    /// what stops a panel being dragged past the edge of the window.
    pub fn resize(&mut self, panel: crate::app::dock::Panel, by: f32, room: f32) {
        if self.dock.side_of(panel).is_a_column() {
            let most = self.max_width_of(panel).min(room.max(self.min_width_of(panel)));
            let width = (self.width_of(panel) + by).clamp(self.min_width_of(panel), most);
            self.set_width_of(panel, width);
        } else {
            let most = room.max(self.min_height_of(panel));
            let height = (self.height_of(panel) + by).clamp(self.min_height_of(panel), most);
            self.set_height_of(panel, height);
        }
    }

    /// Put one panel back to the size it started at, which is what a double click on its divider means.
    pub fn reset_size_of(&mut self, panel: crate::app::dock::Panel) {
        let fresh = Panes::new();
        if self.dock.side_of(panel).is_a_column() {
            self.set_width_of(panel, fresh.width_of(panel));
        } else {
            self.set_height_of(panel, fresh.height_of(panel));
        }
    }
}

impl Default for Panes {
    fn default() -> Self {
        Self::new()
    }
}

/// Read both from the store in one go.
pub fn load(store: &Store) -> (Settings, Panes) {
    load_with(store, &[])
}

/// The same, told the names of the panes plugins contributed, in slot order.
///
/// A window reads its settings before it knows what the plugins contribute — the plugins are read from
/// the same store — so the plain [`load`] is what the first read uses and this is what a later reload
/// uses. A pane whose name is not in the list keeps its manifest's side and size, which is right: there
/// is nothing recorded about it yet.
pub fn load_with(store: &Store, plugin_panes: &[String]) -> (Settings, Panes) {
    let values = store.read_values();
    (Settings::read_from(&values), Panes::read_from_with(&values, plugin_panes))
}

/// Write both to the store in one go, keeping any value in the file that neither of them owns.
pub fn save(store: &Store, settings: &Settings, panes: &Panes) {
    save_with(store, settings, panes, &[]);
}

/// The same, told the names of the panes plugins contributed, in slot order, so a contributed pane's
/// side and size are written against its own name rather than being lost.
pub fn save_with(
    store: &Store,
    settings: &Settings,
    panes: &Panes,
    plugin_panes: &[String],
) {
    let mut values = store.read_values();
    settings.write_into(&mut values);
    panes.write_into_with(&mut values, plugin_panes);
    store.write_values(&values);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_being_written_and_read_back() {
        let settings = Settings {
            font_family: "Courier New".to_owned(),
            font_size: 20.0,
            opacity: 0.4,
            terminal_font_size: 14.0,
            terminal_shell: "pwsh.exe".to_owned(),
            line_numbers: false,
            suggestions: Suggestions::Manual,
            line_endings: LineEndings::Crlf,
            update_check: UpdateCheck::Start,
            exclude: "dist/, vendor/".to_owned(),
            value_tooltip: ValueTooltip::Manual,
            plugin_chrome: false,
            mcp_enabled: true,
            mcp_port: 9001,
            mcp_tools: unluminate_cli::mcp::Shape::Every,
            mcp_areas: "editor,git".to_owned(),
            theme: "themes-bundle-1/monokai-pro".to_owned(),
            accent: "#FF79C6".to_owned(),
            icons: "material".to_owned(),
            ui_font_family: "Segoe UI".to_owned(),
            ui_font_size: 14.0,
            debug_adapters: vec![("lldb".to_owned(), r"C:\tools\codelldb.exe".to_owned())],
        };
        let mut values = Values::new();
        settings.write_into(&mut values);
        assert_eq!(Settings::read_from(&values), settings);
    }

    /// A setting cleared back to its default is **taken out of the file**, not left in it.
    ///
    /// `settings::save_with` merges over what is already there, so a value that was only ever written
    /// when non-empty came back at the next start: choosing a theme and then choosing Unluminate Dark again
    /// would have come up in the first theme tomorrow. Found by driving a real window.
    #[test]
    fn a_setting_put_back_to_its_default_is_taken_out_of_the_file() {
        let mut values = Values::new();
        let mut chosen = Settings::new();
        chosen.theme = "themes-bundle-1/monokai-pro".to_owned();
        chosen.accent = "#FF79C6".to_owned();
        chosen.icons = "classic".to_owned();
        chosen.terminal_shell = "cmd.exe".to_owned();
        chosen.ui_font_family = "Segoe UI".to_owned();
        chosen.debug_adapters.push(("lldb".to_owned(), r"C:\tools\codelldb.exe".to_owned()));
        chosen.write_into(&mut values);
        assert_eq!(values.text("appearance.theme"), Some("themes-bundle-1/monokai-pro"));
        assert_eq!(values.text("debug.lldb"), Some(r"C:\tools\codelldb.exe"));

        // And now put every one of them back to what an Unluminate that has never been run has.
        Settings::new().write_into(&mut values);
        for name in [
            "appearance.theme",
            "appearance.accent",
            "appearance.icons",
            "appearance.ui.font.family",
            "appearance.font.family",
            "terminal.shell",
            "debug.lldb",
            "debug.node",
        ] {
            assert_eq!(values.text(name), None, "{name} should be gone from the file");
        }
        // Read back, the file asks for the defaults rather than for what was chosen before.
        assert_eq!(Settings::read_from(&values), Settings::new());
    }

    /// The adapter paths follow `terminal.shell`'s rule exactly: nothing is written until one has
    /// been chosen, because a settings file is copied between machines and a path that exists on one
    /// of them is worse than no line at all.
    #[test]
    fn an_adapter_path_is_only_written_once_it_has_been_chosen() {
        let mut values = Values::new();
        Settings::new().write_into(&mut values);
        assert_eq!(values.text("debug.lldb"), None);
        assert_eq!(values.text("debug.node"), None);
        assert!(Settings::new().debug_adapter("lldb").is_none(), "empty means what this machine has");

        let chosen = Settings::read_from(&Values::parse(r"debug.lldb = C:\tools\codelldb.exe"));
        assert_eq!(chosen.debug_adapter("lldb"), Some(r"C:\tools\codelldb.exe"));
        assert_eq!(chosen.debug_adapter("node"), None);

        let blank = Settings::read_from(&Values::parse("debug.lldb =    "));
        assert!(blank.debug_adapter("lldb").is_none(), "a blank line is not a path");
    }

    #[test]
    fn the_mcp_server_is_off_until_somebody_turns_it_on() {
        // Not timidity: the server an agent launches over its own pipes needs neither this setting
        // nor a port, and it is what the install buttons write. A fixed open port that will run
        // `terminal send` should be a thing somebody chose.
        let fresh = Settings::new();
        assert!(!fresh.mcp_enabled);
        assert_eq!(fresh.mcp_port, unluminate_cli::mcp::DEFAULT_PORT);
        assert_eq!(fresh.mcp_tools, unluminate_cli::mcp::Shape::Grouped);
    }

    #[test]
    fn a_hand_edited_port_is_brought_inside_its_limits_rather_than_refused() {
        // The rule every other setting keeps: an extreme is clamped and only a value that is not a
        // number at all is a mistake.
        let low = Settings::read_from(&Values::parse("mcp.port = 22
"));
        assert_eq!(low.mcp_port, unluminate_cli::mcp::MIN_PORT);
        let high = Settings::read_from(&Values::parse("mcp.port = 999999
"));
        assert_eq!(high.mcp_port, u16::MAX);
        let nonsense = Settings::read_from(&Values::parse("mcp.port = banana
"));
        assert_eq!(nonsense.mcp_port, unluminate_cli::mcp::DEFAULT_PORT, "an unreadable line is no line");
        let shape = Settings::read_from(&Values::parse("mcp.tools = every
"));
        assert_eq!(shape.mcp_tools, unluminate_cli::mcp::Shape::Every);
        let unknown = Settings::read_from(&Values::parse("mcp.tools = clever
"));
        assert_eq!(unknown.mcp_tools, unluminate_cli::mcp::Shape::Grouped, "a word this version does not have is ignored");
    }

    #[test]
    fn no_shell_chosen_means_the_one_this_machine_says_the_person_has() {
        // `task-1670`: the setting exists so that a person can ask for `cmd.exe` back, and an empty one
        // has to mean "whatever the machine says" rather than a program with no name.
        let settings = Settings::new();
        assert_eq!(settings.shell(), None);

        let mut values = Values::new();
        settings.write_into(&mut values);
        assert_eq!(values.text("terminal.shell"), None, "nothing is written until it is chosen");

        let chosen = Settings::read_from(&Values::parse("terminal.shell = cmd.exe
"));
        assert_eq!(chosen.shell().as_deref(), Some("cmd.exe"));
        let blank = Settings::read_from(&Values::parse("terminal.shell =   
"));
        assert_eq!(blank.shell(), None, "a line with nothing after it is not a shell");
    }

    #[test]
    fn suggestions_default_to_automatic_and_a_value_this_version_does_not_have_is_ignored() {
        assert_eq!(Settings::new().suggestions, Suggestions::Automatic);
        assert!(Settings::new().suggestions.is_automatic());
        let manual = Settings::read_from(&Values::parse("editor.suggestions = manual
"));
        assert_eq!(manual.suggestions, Suggestions::Manual);
        // A word this version has never heard of leaves the default alone rather than switching the
        // feature off by accident, which is the answer `plugin.kind` gives to the same question.
        let odd = Settings::read_from(&Values::parse("editor.suggestions = telepathy
"));
        assert_eq!(odd.suggestions, Suggestions::Automatic);
        for value in [Suggestions::Automatic, Suggestions::Manual] {
            assert_eq!(Suggestions::parse(value.name()), Some(value));
        }
    }

    #[test]
    fn a_value_outside_its_limits_is_brought_back_inside() {
        let values = Values::parse(
            "appearance.background.opacity = 12\nappearance.font.size = 900\nterminal.font.size = 0\n",
        );
        let settings = Settings::read_from(&values);
        assert_eq!(settings.opacity, 1.0, "the background cannot be more than fully opaque");
        assert_eq!(settings.font_size, 144.0);
        assert_eq!(settings.terminal_font_size, 6.0);
    }

    #[test]
    fn stepping_the_font_size_walks_the_sizes_the_dialog_offers() {
        assert_eq!(step_font_size(16.0, true), 20.0);
        assert_eq!(step_font_size(16.0, false), 13.0);
        assert_eq!(step_font_size(64.0, true), 64.0, "the top of the list stays there");
        assert_eq!(step_font_size(9.0, false), 9.0, "and so does the bottom");
    }

    #[test]
    fn stepping_from_a_size_that_is_not_in_the_list_still_moves() {
        // A pinch, or a hand edited settings file, can leave the size between two of the offered
        // ones. Pressing the key has to move, and has to land on a size the dialog can show.
        assert_eq!(step_font_size(17.0, true), 20.0);
        assert_eq!(step_font_size(17.0, false), 16.0);
        assert_eq!(step_font_size(200.0, false), 64.0, "above everything offered");
        assert_eq!(step_font_size(2.0, true), 9.0, "below everything offered");
    }

    #[test]
    fn pane_sizes_survive_being_written_and_read_back() {
        let mut dock = crate::app::dock::Layout::new();
        dock.dock(crate::app::dock::Panel::Terminal, crate::app::dock::Side::Right, None);
        let panes = Panes {
            // Two contributed panes are named below, so only the first two slots are remembered; a
            // slot no plugin is in keeps its default, because there is no pane whose size to record.
            plugin_widths: [420.0, 300.0, PLUGIN_PANE_WIDTH, PLUGIN_PANE_WIDTH],
            plugin_heights: [280.0, 260.0, PLUGIN_PANE_HEIGHT, PLUGIN_PANE_HEIGHT],
            plugin_zooms: [1.35, 0.85, DEFAULT_ZOOM, DEFAULT_ZOOM],
            explorer_zoom: 1.15,
            explorer_width: 320.0,
            explorer_height: 220.0,
            terminal_height: 400.0,
            terminal_width: 380.0,
            run_height: 300.0,
            run_width: 440.0,
            debug_height: 340.0,
            debug_width: 560.0,
            dock,
            preview_fraction: 0.3,
            find_split: 0.6,
            references_split: 0.4,
        };
        let mut values = Values::new();
        // Two contributed panes, named the way the settings file names them, so this also pins that a
        // pane's width follows its own name rather than its slot number.
        let contributed = vec!["agent-tasks/board".to_owned(), "chat/thread".to_owned()];
        panes.write_into_with(&mut values, &contributed);
        assert_eq!(Panes::read_from_with(&values, &contributed), panes);
        assert_eq!(
            values.text("panes.agent-tasks/board.width"),
            Some("420"),
            "the first contributed pane's width is recorded against its own name"
        );
        // Read back with the two plugins the other way round: each pane keeps its own width, which is the
        // whole reason the name rather than the slot is the key.
        let swapped = vec!["chat/thread".to_owned(), "agent-tasks/board".to_owned()];
        let read = Panes::read_from_with(&values, &swapped);
        assert_eq!(read.plugin_widths[0], 300.0, "chat's pane kept chat's width");
        assert_eq!(read.plugin_widths[1], 420.0, "and the board kept the board's");
        // And so does its zoom, which is `task-1771`'s per-pane one: it follows the pane rather than the
        // slot, exactly as the two measurements do.
        assert_eq!(read.plugin_zooms[0], 0.85);
        assert_eq!(read.plugin_zooms[1], 1.35);
    }

    #[test]
    fn a_panel_is_resized_by_the_measurement_the_side_it_is_on_reads() {
        use crate::app::dock::{Panel, Side};
        let mut panes = Panes::new();
        // At the bottom, where it starts, a drag moves its height.
        panes.resize(Panel::Terminal, 40.0, 600.0);
        assert_eq!(panes.terminal_height, TERMINAL_HEIGHT + 40.0);
        assert_eq!(panes.terminal_width, TERMINAL_WIDTH, "the other measurement is untouched");
        // On the right it is a column, so the same drag moves its width.
        panes.dock.dock(Panel::Terminal, Side::Right, None);
        panes.resize(Panel::Terminal, 40.0, 900.0);
        assert_eq!(panes.terminal_width, TERMINAL_WIDTH + 40.0);
        assert_eq!(panes.terminal_height, TERMINAL_HEIGHT + 40.0, "and that one is untouched now");
        panes.reset_size_of(Panel::Terminal);
        assert_eq!(panes.terminal_width, TERMINAL_WIDTH);
    }

    #[test]
    fn a_pane_dragged_past_its_limit_comes_back_inside_on_the_next_run() {
        let values = Values::parse("panes.explorer.width = 4000\npanes.terminal.height = 2\n");
        let panes = Panes::read_from(&values);
        assert_eq!(panes.explorer_width, EXPLORER_MAX);
        assert_eq!(panes.terminal_height, TERMINAL_MIN);
    }

    #[test]
    fn the_font_setting_becomes_a_style_change_that_names_only_the_family_and_the_size() {
        let settings = Settings { font_family: "Menlo".to_owned(), ..Settings::new() };
        let change = settings.as_style_change();
        assert_eq!(change.family.as_deref(), Some("Menlo"));
        assert_eq!(change.size, Some(16.0));
        assert_eq!(change.bold, None, "a font setting must not touch bold");
        assert_eq!(change.color, None, "or the colour");
    }

    #[test]
    fn every_page_is_listed_under_a_group_and_has_sections() {
        for page in Page::ALL {
            assert!(!page.title().is_empty());
            assert!(!page.sections().is_empty(), "{} should have sections", page.title());
        }
        // Every page but one is listed under a heading. `Plugins` has none, because it is one page
        // rather than a group with pages under it, which is how the reference editor lists it too, and the list
        // draws a page with no heading at the left margin instead of indented under one.
        let ungrouped: Vec<&str> =
            Page::ALL.into_iter().filter(|page| page.group().is_empty()).map(Page::title).collect();
        assert_eq!(ungrouped, vec!["Plugins"]);
    }

    #[test]
    fn the_search_box_matches_a_page_by_its_name_its_group_or_a_section_in_it() {
        assert!(Page::Appearance.matches(""), "an empty search shows every page");
        assert!(Page::Appearance.matches("appear"));
        assert!(Page::Appearance.matches("behavior"));
        assert!(Page::Appearance.matches("background"), "a section inside the page counts");
        assert!(Page::Terminal.matches("font"), "both pages have a Font section");
        assert!(!Page::Terminal.matches("background"));
    }
}
