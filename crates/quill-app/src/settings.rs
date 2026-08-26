//! What the Settings window holds, and what is remembered between runs.
//!
//! Two structures, because they are two different things. [`Settings`] is what a person chooses in
//! `Edit -> Settings`: the editor's font, the background opacity and the terminal's font size.
//! [`Panes`] is where the draggable dividers were left, which nobody chooses in a dialog but which should
//! still be there next time.
//!
//! Both are read from and written to the same file through [`crate::services::store`]. Names in the file
//! are grouped with dots so that the file reads like the dialog: `appearance.font.size` is the size on
//! the Appearance page under the Font heading.

use crate::services::store::{Store, Values};

/// The width the explorer starts at, and the smallest and largest it can be dragged to.
pub const EXPLORER_WIDTH: f32 = 248.0;
pub const EXPLORER_MIN: f32 = 150.0;
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

/// How opaque the background is when Quill starts. Not fully opaque, so the transparency is visible
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

/// Whether the completion popup appears without being asked for.
///
/// Two values rather than three, and the reason is worth writing down: the person this setting
/// exists for is the one who found the popup arriving unasked distracting, and for them `manual` is
/// already the off switch — `Ctrl+Space`, the `Complete Word` entry and the command line all still
/// work, and nothing appears until one of them is asked. A third value meaning "off altogether"
/// would take away a key that never interrupts anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Suggestions {
    /// The popup opens while you type, at a two character stem. IntelliJ's own default.
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

/// One page of the Settings window, and the group it is listed under.
///
/// The list on the left of the window is built from this, so adding a page is one variant and one match
/// arm rather than a change to the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// The editor's font and the window's background.
    #[default]
    Appearance,
    /// The editing area itself: the gutter, and the colour scheme code is set in.
    Editor,
    /// The plugins that are installed, and the marketplace they came from.
    Plugins,
    /// The terminal at the bottom of the window.
    Terminal,
    /// The Model Context Protocol server, which is how an AI agent drives Quill.
    Mcp,
}

impl Page {
    pub const ALL: [Page; 5] =
        [Page::Appearance, Page::Editor, Page::Plugins, Page::Terminal, Page::Mcp];

    /// The name in the list on the left, and the last part of the heading.
    pub fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Editor => "Editor",
            Page::Plugins => "Plugins",
            Page::Terminal => "Terminal",
            Page::Mcp => "MCP",
        }
    }

    /// The heading the page is listed under, which is also the first part of the breadcrumb.
    pub fn group(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance & Behavior",
            Page::Editor => "Editor",
            // No heading of its own, the way IntelliJ lists Plugins: it is one page rather than a
            // group with pages under it.
            Page::Plugins => "",
            Page::Terminal => "Tools",
            // The same heading as the terminal: both are a way of reaching something outside the
            // editor from inside it.
            Page::Mcp => "Tools",
        }
    }

    /// The headings inside the page. They are what the search box matches on as well as being drawn.
    pub fn sections(self) -> &'static [&'static str] {
        match self {
            Page::Appearance => &["Font", "Background"],
            Page::Editor => &["Gutter", "Suggestions"],
            Page::Plugins => &["Marketplace", "Installed", "Colour Scheme", "Syntax"],
            Page::Terminal => &["Font", "Shell"],
            Page::Mcp => &["Install", "Server", "Configuration"],
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
    /// `quill_terminal::session` decides and which is PowerShell on Windows.
    pub terminal_shell: String,
    /// Whether the editing area has a column of line numbers down its left.
    pub line_numbers: bool,
    /// Whether the completion popup arrives while you type, or waits to be asked.
    pub suggestions: Suggestions,
    /// Whether this Quill hosts the MCP server over HTTP, so an agent can reach it at a URL.
    pub mcp_enabled: bool,
    /// The port it listens on when it does.
    pub mcp_port: u16,
    /// How many tools the catalogue is cut into for an agent. See `quill_cli::mcp::tools`.
    pub mcp_tools: quill_cli::mcp::Shape,
    /// Where each debug adapter lives, when this machine keeps one somewhere Quill would not look.
    ///
    /// `debug.lldb`, `debug.node` — one entry per name in `plugins::DEBUGGERS`, and empty meaning
    /// "whatever this machine has", which is `Settings::shell()`'s sentence made once more. It is a
    /// path rather than a preference, so a settings file copied to another machine names nothing.
    pub debug_adapters: Vec<(String, String)>,
}

impl Settings {
    /// The settings a Quill that has never been run has. The family is decided by the renderer, because
    /// it depends on what the system has installed, so it is left empty here and filled in by the window.
    pub fn new() -> Self {
        Self {
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            opacity: DEFAULT_OPACITY,
            terminal_font_size: 13.0,
            // Empty rather than a name, because which shell is right is a question about the machine
            // Quill is running on, and the settings file is copied between machines.
            terminal_shell: String::new(),
            // On, because a line number is useful in prose as well as in code and a person who does
            // not want one can put it away from the gutter's own menu.
            line_numbers: true,
            // On, which is what IntelliJ's own "Show suggestions as you type" is, and what the
            // ticket asked for: suggestions that arrive rather than ones you have to remember to
            // ask for.
            suggestions: Suggestions::Automatic,
            // Off, and the reason is worth writing down rather than being read off as timidity.
            // The MCP server an agent launches over its own pipes needs no port and no setting: it
            // lives as long as the conversation and nothing is listening when nobody is asking,
            // which is what `Settings -> Tools -> MCP`'s install buttons set up. A fixed open port
            // that will run `terminal send` for anything that can reach it is a different
            // proposition, and it should be a thing somebody turned on rather than a thing they
            // were given.
            mcp_enabled: false,
            mcp_port: quill_cli::mcp::DEFAULT_PORT,
            mcp_tools: quill_cli::mcp::Shape::default(),
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
        if let Some(chosen) = values.text("editor.suggestions").and_then(Suggestions::parse) {
            settings.suggestions = chosen;
        }
        if let Some(on) = values.flag("mcp.enabled") {
            settings.mcp_enabled = on;
        }
        if let Some(port) = values.number("mcp.port") {
            settings.mcp_port = clamp_port(port);
        }
        if let Some(shape) =
            values.text("mcp.tools").and_then(quill_cli::mcp::Shape::parse)
        {
            settings.mcp_tools = shape;
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
        if !self.font_family.is_empty() {
            values.set("appearance.font.family", self.font_family.clone());
        }
        values.set("appearance.font.size", format!("{:.0}", self.font_size));
        values.set("appearance.background.opacity", format!("{:.3}", self.opacity));
        values.set("terminal.font.size", format!("{:.0}", self.terminal_font_size));
        // Written only once it has been chosen, so the file does not name a shell on every machine it
        // is copied to. An empty line would read as a shell called nothing.
        if !self.terminal_shell.is_empty() {
            values.set("terminal.shell", self.terminal_shell.clone());
        }
        values.set("editor.line_numbers", if self.line_numbers { "true" } else { "false" });
        values.set("editor.suggestions", self.suggestions.name());
        values.set("mcp.enabled", if self.mcp_enabled { "true" } else { "false" });
        values.set("mcp.port", self.mcp_port.to_string());
        values.set("mcp.tools", self.mcp_tools.name());
        // Written only once one has been chosen, exactly as the shell is, so a settings file does
        // not name a path that exists on one machine and not on the next.
        for (name, path) in &self.debug_adapters {
            values.set(&format!("debug.{name}"), path.clone());
        }
    }

    /// The path this machine's settings give for the debug adapter called `name`, or nothing when
    /// they name none and Quill should look for it itself.
    ///
    /// [`Settings::shell`]'s sentence, made once more and in one function rather than the same
    /// `is_empty` test wherever an adapter is started.
    pub fn debug_adapter(&self, name: &str) -> Option<&str> {
        self.debug_adapters
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, path)| path.as_str())
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
    pub fn as_style_change(&self) -> quill_core::StyleChange {
        quill_core::StyleChange {
            family: (!self.font_family.is_empty()).then(|| self.font_family.clone()),
            size: Some(self.font_size),
            ..quill_core::StyleChange::default()
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
    port.clamp(quill_cli::mcp::MIN_PORT as f32, u16::MAX as f32) as u16
}

/// Where the draggable dividers were left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panes {
    /// How wide the explorer is.
    pub explorer_width: f32,
    /// How tall the terminal tile is.
    pub terminal_height: f32,
    /// How tall the run tile is. Its own measurement — see [`RUN_HEIGHT`].
    pub run_height: f32,
    /// How tall the debug tile is. Its own too — see [`DEBUG_HEIGHT`].
    pub debug_height: f32,
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
            terminal_height: TERMINAL_HEIGHT,
            run_height: RUN_HEIGHT,
            debug_height: DEBUG_HEIGHT,
            preview_fraction: 0.5,
            find_split: crate::components::find_in_files::SPLIT,
            references_split: crate::components::references::SPLIT,
        }
    }

    pub fn read_from(values: &Values) -> Self {
        let mut panes = Self::new();
        if let Some(width) = values.number("panes.explorer.width") {
            panes.explorer_width = width.clamp(EXPLORER_MIN, EXPLORER_MAX);
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
        values.set("panes.explorer.width", format!("{:.0}", self.explorer_width));
        values.set("panes.terminal.height", format!("{:.0}", self.terminal_height));
        values.set("panes.run.height", format!("{:.0}", self.run_height));
        values.set("panes.debug.height", format!("{:.0}", self.debug_height));
        values.set("panes.preview.fraction", format!("{:.3}", self.preview_fraction));
        values.set("panes.find.split", format!("{:.3}", self.find_split));
        values.set("panes.references.split", format!("{:.3}", self.references_split));
    }
}

impl Default for Panes {
    fn default() -> Self {
        Self::new()
    }
}

/// Read both from the store in one go.
pub fn load(store: &Store) -> (Settings, Panes) {
    let values = store.read_values();
    (Settings::read_from(&values), Panes::read_from(&values))
}

/// Write both to the store in one go, keeping any value in the file that neither of them owns.
pub fn save(store: &Store, settings: &Settings, panes: &Panes) {
    let mut values = store.read_values();
    settings.write_into(&mut values);
    panes.write_into(&mut values);
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
            mcp_enabled: true,
            mcp_port: 9001,
            mcp_tools: quill_cli::mcp::Shape::Every,
            debug_adapters: vec![("lldb".to_owned(), r"C:\tools\codelldb.exe".to_owned())],
        };
        let mut values = Values::new();
        settings.write_into(&mut values);
        assert_eq!(Settings::read_from(&values), settings);
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
        assert_eq!(fresh.mcp_port, quill_cli::mcp::DEFAULT_PORT);
        assert_eq!(fresh.mcp_tools, quill_cli::mcp::Shape::Grouped);
    }

    #[test]
    fn a_hand_edited_port_is_brought_inside_its_limits_rather_than_refused() {
        // The rule every other setting keeps: an extreme is clamped and only a value that is not a
        // number at all is a mistake.
        let low = Settings::read_from(&Values::parse("mcp.port = 22
"));
        assert_eq!(low.mcp_port, quill_cli::mcp::MIN_PORT);
        let high = Settings::read_from(&Values::parse("mcp.port = 999999
"));
        assert_eq!(high.mcp_port, u16::MAX);
        let nonsense = Settings::read_from(&Values::parse("mcp.port = banana
"));
        assert_eq!(nonsense.mcp_port, quill_cli::mcp::DEFAULT_PORT, "an unreadable line is no line");
        let shape = Settings::read_from(&Values::parse("mcp.tools = every
"));
        assert_eq!(shape.mcp_tools, quill_cli::mcp::Shape::Every);
        let unknown = Settings::read_from(&Values::parse("mcp.tools = clever
"));
        assert_eq!(unknown.mcp_tools, quill_cli::mcp::Shape::Grouped, "a word this version does not have is ignored");
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
        let panes = Panes {
            explorer_width: 320.0,
            terminal_height: 400.0,
            run_height: 300.0,
            debug_height: 340.0,
            preview_fraction: 0.3,
            find_split: 0.6,
            references_split: 0.4,
        };
        let mut values = Values::new();
        panes.write_into(&mut values);
        assert_eq!(Panes::read_from(&values), panes);
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
        // rather than a group with pages under it, which is how IntelliJ lists it too, and the list
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
