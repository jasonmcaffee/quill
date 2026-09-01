//! The plugins: what one is, where they come from, and which one claims a file.
//!
//! ## A plugin is data, not code
//!
//! `task-1649` asks for plugins that give a file type an icon, identify its keywords, function
//! names, imports and comments, and supply a theme. That is a **description of a language**, not a
//! program. So a plugin is a folder holding a manifest, an icon and the words that make up a
//! language, and loading one is reading a file. Nothing is executed.
//!
//! The two alternatives were considered and both are the right answer to a question this is not
//! asking yet.
//!
//! A **dynamic library** would let a plugin run arbitrary Rust. It also means an unstable interface
//! across a `dlopen` boundary — a Rust structure passed over one is undefined behaviour unless both
//! sides were built by the same compiler with the same flags — so every plugin would have to be
//! rebuilt for every release of Quill, and a plugin that crashes takes the editor with it. For
//! "colour these keywords" that is a great deal of risk bought for nothing.
//!
//! **WebAssembly** answers both of those and costs a runtime, plus a host interface that has to be
//! designed, versioned and documented before the first plugin can be written. It is the right answer
//! the day a plugin wants to *do* something: run a formatter, talk to a language server, add a tool
//! window.
//!
//! So the seam is named now and left empty. `plugin.kind` is read and checked, and a manifest saying
//! anything but `language` is refused with a message rather than half-loaded. That is the line a
//! later version widens.
//!
//! ## A language that has a picture
//!
//! `task-1660` asks for a Mermaid plugin, and Mermaid **is** a language: it has keywords, comments,
//! strings and a file extension, and colouring `.mmd` source is worth having on its own. So it is an
//! ordinary `language` plugin, and the seam above stays exactly where it was.
//!
//! It carries one new key, `language.renders`, which names a renderer that is **built into Quill**.
//! Nothing is loaded from the plugin and nothing is executed: the manifest is data saying "files of
//! this language have a picture, and this is which picture", and the code that draws it shipped with
//! the binary. The value is checked against [`RENDERERS`], and a manifest naming one this version
//! does not have is refused with a message — the same rule `plugin.kind` already keeps.
//!
//! What it buys is that switching the plugin off actually withdraws the feature: the window asks
//! [`Plugins::renders`] before it draws a diagram anywhere, so `.mmd` files stop being drawn and
//! mermaid blocks in Markdown go back to being code, in the same frame.
//!
//! ## The manifest
//!
//! `plugin.conf`, in the same `name = value` format the settings file already uses, read by the same
//! [`crate::services::store::Values`]. No new dependency, and a plugin can be read and corrected in
//! a text editor, which is fitting in a text editor.

use std::path::{Path, PathBuf};

use quill_core::symbols::SymbolKind;
use quill_core::syntax::{Grammar, ImportStyle, PathRoot, Token};
use quill_core::Color;

use crate::services::store::{Store, Values};

/// The folder under the settings folder that installed plugins live in.
const FOLDER: &str = "plugins";
/// The file inside a plugin folder that describes it.
const MANIFEST: &str = "plugin.conf";
/// The picture a plugin puts in front of its files.
const ICON: &str = "icon.png";

/// The renderers built into this version of Quill that a plugin may name.
///
/// Checked rather than taken on trust, so a manifest asking for a picture Quill cannot draw says so
/// plainly instead of loading as a language whose files quietly never draw.
pub const RENDERERS: &[&str] = &["mermaid"];

/// The project detectors built into this version of Quill that a plugin's `run.project` may name.
///
/// Checked the same way [`RENDERERS`] is, and for the same reason: a manifest asking for a detector
/// Quill does not have should say so plainly rather than load as a language whose projects are
/// quietly never noticed. `services::run_configurations::detect` is what each one does.
///
/// This is the answer to the question `task-1683` opens with — should running node mean a Node
/// plugin? No: node is how JavaScript runs, and a plugin with no language, no extensions and no
/// tokens, existing to carry one line of data, is not a plugin. The JavaScript manifest carries
/// that line itself, exactly as Mermaid named a built-in renderer rather than widening
/// `plugin.kind`.
pub const PROJECT_RUNNERS: &[&str] = &["cargo", "npm"];

/// The debuggers built into this version of Quill that a plugin's `debug.adapter` may name.
///
/// The third registry of this shape, checked the same way and for the same reason: a manifest naming
/// a debugger Quill cannot drive should say so plainly rather than load as a language whose files
/// quietly offer a Debug button that never works.
///
/// **Which debugger a language uses is data in the plugin, and the debugger itself is code in
/// Quill.** `services::debuggers` is what each name knows how to find and how to start — where
/// `lldb-dap` lives on `PATH`, how to translate a run configuration into that adapter's own launch
/// shape — so the most a third-party manifest can do is name an adapter that shipped in the binary,
/// visibly. Nothing in a plugin is executed and nothing is ever fetched.
pub const DEBUGGERS: &[&str] = &["lldb", "node"];

/// The UI providers built into this version of Quill that a plugin's `ui.provider` may name.
///
/// The fourth registry of this shape, checked the same way and for the same reason as [`RENDERERS`],
/// [`PROJECT_RUNNERS`] and [`DEBUGGERS`]: a manifest naming a provider Quill does not have should say
/// so plainly rather than load as a plugin whose pane is permanently empty.
///
/// **What a plugin contributes is data and the code that draws it is Quill's.** A manifest says there
/// is a pane, where it docks, what its button looks like and what its menu holds; the drawing shipped
/// with the binary. So the most a manifest can do is name a provider that is already here, visibly,
/// and nothing in a plugin is executed.
pub const UI_PROVIDERS: &[&str] = &["agent-tasks"];

/// The renderers a plugin's `ui.chrome` may name for the decoration `egui` cannot draw.
///
/// The fifth registry of this shape, checked the same way and for the same reason as the four above. A
/// manifest saying `ui.chrome = vello` asks for the soft shadows, inset shadows, gradients and rounded
/// clips of `services::vello_canvas`; a manifest naming anything else is refused with the list rather than
/// loading as a plugin whose pane is quietly flat.
///
/// **It is off unless a manifest asks**, which is the rule `language.word_characters`, `language.types`,
/// `language.markup` and every import key already keep, so no plugin that shipped before this changes by a
/// pixel. Switching it off in the manifest really withdraws the decoration, in the same frame, which is
/// the property `Plugins::renders` has for a Mermaid diagram.
pub const CHROME: &[&str] = &["vello"];

/// The icons a `pane.icon` may name, drawn by [`crate::theme::icon`].
///
/// Checked for the same reason the three registries above are checked: a rail button drawn as nothing
/// is worse than a manifest that was refused with the list of icons in the message.
pub const PANE_ICONS: &[&str] =
    &["board", "folder", "terminal", "run", "bug", "clock", "branch", "tick", "plus", "image"];

/// The conditions a `pane.applies` may name.
///
/// Quill's answer to VS Code's `when` expressions, which are the most copied part of its contribution
/// model and the hardest to keep tested. A control that cannot apply is absent here, and the question
/// is a function rather than an expression, so there are two named conditions and a list to check
/// against instead of a language to parse.
pub const PANE_CONDITIONS: &[&str] = &["always", "in_project"];

/// The kind of plugin.
///
/// Two, and the second one is what `tasks/ui-plugin-architecture.md` widened the seam for. A third is
/// still refused rather than half-loaded, which is what the field was added for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A description of a language: extensions, a grammar, an icon and a colour scheme.
    Language,
    /// A plugin that draws: it contributes a rail button and a pane, a tab in the editing area, a
    /// menu, and a page in Settings. The arrangement is data in the manifest and the drawing is code
    /// in Quill, named by `ui.provider`.
    Ui,
}

impl Kind {
    /// The word the manifest, the settings file and the command line call it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Language => "language",
            Kind::Ui => "ui",
        }
    }
}

/// A colour scheme: one colour per kind of token.
///
/// **It colours the tokens and not the background.** Dracula's own `#282A36` is not used, and
/// Quill's `theme::color::EDITOR` stays, because the window letting the desktop show through is the
/// whole character of the product and a scheme that repaints the editing area opaque would take that
/// away in exchange for being a shade nearer a screenshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTheme {
    pub name: String,
    /// One colour a token, in the order of [`Token::ALL`].
    colours: Vec<(Token, Color)>,
}

impl SyntaxTheme {
    pub fn colour(&self, token: Token) -> Option<Color> {
        self.colours.iter().find(|(known, _)| *known == token).map(|(_, colour)| *colour)
    }

    pub fn is_empty(&self) -> bool {
        self.colours.is_empty()
    }
}

/// Which group of the rail a pane's button goes in.
///
/// The rail's two groups say what a panel **is** rather than where it happens to be: the top group
/// holds lists and the bottom holds tiles with a character grid in them. That is the distinction
/// `components::activity_bar` already draws, and a contributed pane joins one of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailGroup {
    Top,
    Bottom,
}

impl RailGroup {
    pub fn name(self) -> &'static str {
        match self {
            RailGroup::Top => "top",
            RailGroup::Bottom => "bottom",
        }
    }
}

/// A pane a plugin contributes, and the button in the rail that shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneContribution {
    /// The name the command line and the settings file call it. Lower case, one word.
    pub id: String,
    /// What a person reads in the rail's tooltip and on the pane's own header.
    pub label: String,
    /// Which drawn icon goes in the rail, from [`PANE_ICONS`].
    pub icon: String,
    pub group: RailGroup,
    /// The side it docks to the first time it is shown.
    pub side: crate::app::dock::Side,
    /// The two measurements every panel carries, because one number cannot be both: a width for when
    /// it is a column at the side, and a height for when it is in a strip.
    pub width: f32,
    pub height: f32,
    /// The condition under which the button is drawn at all, from [`PANE_CONDITIONS`].
    pub applies: String,
}

/// A tab in the editing area a plugin contributes.
///
/// It has no path on disk, is never modified and cannot be saved, which are the four answers a picture
/// tab already gives to the four questions the window asks a tab.
#[derive(Debug, Clone, PartialEq)]
pub struct TabContribution {
    pub id: String,
    pub label: String,
}

/// One row of a plugin's menu: something to do, a separator, or a menu inside a menu.
///
/// Recursive, because `menu.submenu.<id>.submenu.<other>` is a submenu inside a submenu and
/// `actions::Entry::Submenu` already holds a `Vec<Entry>`.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    /// `command=Name`: the name a person reads, and the command handed to the provider.
    Command { command: String, label: String },
    /// A lone `-` in the list.
    Separator,
    Submenu { label: String, items: Vec<MenuItem> },
}

/// A menu a plugin contributes, added after the six Quill has.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuContribution {
    pub name: String,
    pub items: Vec<MenuItem>,
}

/// A page in the Settings window a plugin contributes.
#[derive(Debug, Clone, PartialEq)]
pub struct PageContribution {
    pub name: String,
    pub icon: String,
}

/// Everything one plugin adds to the window.
///
/// A value on the plugin rather than four questions asked of it, so the rail, the dock, the menus, the
/// tab strip and the Settings window all read one thing and none of them can disagree with the others
/// about what was contributed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contributions {
    /// The name in [`UI_PROVIDERS`] of the code that fills the pane, the tab and the page.
    pub provider: Option<String>,
    /// The name in [`CHROME`] of the renderer this plugin's decoration is drawn with, when it asks for one.
    pub chrome: Option<String>,
    pub pane: Option<PaneContribution>,
    pub tab: Option<TabContribution>,
    pub menu: Option<MenuContribution>,
    pub page: Option<PageContribution>,
}

impl Contributions {
    /// True when this manifest adds nothing at all, which is what makes a `ui` plugin unreachable and
    /// is therefore refused.
    pub fn is_empty(&self) -> bool {
        self.pane.is_none() && self.tab.is_none() && self.menu.is_none() && self.page.is_none()
    }
}

/// One contribution, with the plugin it came from.
///
/// A pane, a tab and a page are all reached by `<plugin id>/<contribution id>` rather than by an index,
/// because the set is decided when the manifests are read rather than at compile time. That is the one
/// property `dock::Panel`'s four variants could not have.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface<T> {
    /// The plugin's `plugin.id`.
    pub plugin: String,
    /// The name in [`UI_PROVIDERS`] of the code that fills it.
    pub provider: String,
    pub what: T,
}

impl<T> Surface<T> {
    /// The name the settings file, the dock and the command line call this contribution.
    pub fn key(&self, id: &str) -> String {
        format!("{}/{id}", self.plugin)
    }
}

/// Everything every enabled plugin contributes, worked out once when the plugins are loaded.
///
/// One value rather than a question asked of each plugin every frame: the rail, the dock, the menus,
/// the tab strip and the Settings window all read it, so none of them can disagree with the others
/// about what is contributed. Rebuilt by [`Plugins::set_enabled`], which is what makes switching a
/// plugin off withdraw every contribution in the same frame — the rule `Plugins::renders` already
/// keeps for a Mermaid diagram.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Surfaces {
    pub panes: Vec<Surface<PaneContribution>>,
    pub tabs: Vec<Surface<TabContribution>>,
    pub menus: Vec<Surface<MenuContribution>>,
    pub pages: Vec<Surface<PageContribution>>,
    /// The renderer each plugin asked for, as `(plugin.id, the name from `CHROME`)`.
    ///
    /// **The name is kept, not thrown away for a boolean.** There is one renderer today, so a `Vec<String>`
    /// of ids would have worked and would have been a lie the moment a second name were added to
    /// [`CHROME`]: every plugin would still have gone to `vello_canvas`, and the check would have been a
    /// yes-or-no dressed up as a choice. `Surfaces::chrome_for` answers with the name, and the window
    /// matches on it.
    ///
    /// Here rather than asked of the manifest at drawing time, for the reason the four lists above are
    /// here: switching a plugin off has to withdraw its decoration in the same frame it withdraws its
    /// pane, and one value everything reads is what makes that impossible to get wrong.
    pub chrome: Vec<(String, String)>,
}

impl Surfaces {
    /// The pane named `<plugin>/<pane>`.
    pub fn pane(&self, key: &str) -> Option<&Surface<PaneContribution>> {
        self.panes.iter().find(|surface| surface.key(&surface.what.id) == key)
    }

    pub fn tab(&self, key: &str) -> Option<&Surface<TabContribution>> {
        self.tabs.iter().find(|surface| surface.key(&surface.what.id) == key)
    }

    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
            && self.tabs.is_empty()
            && self.menus.is_empty()
            && self.pages.is_empty()
    }

    /// Which renderer this plugin asked for, if it asked and is switched on.
    pub fn chrome_for(&self, plugin: &str) -> Option<&str> {
        self.chrome
            .iter()
            .find(|(id, _)| id == plugin)
            .map(|(_, renderer)| renderer.as_str())
    }

    /// The provider named by the plugin with this id, from whichever of its contributions names it.
    ///
    /// Every contribution of one plugin carries the same provider, so any of them answers; asking the
    /// panes first is only because most plugins contribute one.
    pub fn provider_of(&self, plugin: &str) -> Option<String> {
        self.panes
            .iter()
            .map(|surface| (&surface.plugin, &surface.provider))
            .chain(self.tabs.iter().map(|surface| (&surface.plugin, &surface.provider)))
            .chain(self.menus.iter().map(|surface| (&surface.plugin, &surface.provider)))
            .chain(self.pages.iter().map(|surface| (&surface.plugin, &surface.provider)))
            .find(|(id, _)| id.as_str() == plugin)
            .map(|(_, provider)| provider.clone())
    }

    /// Every plugin that contributes anything, once each, in the order the plugins are listed.
    pub fn plugins(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for id in self
            .panes
            .iter()
            .map(|surface| &surface.plugin)
            .chain(self.tabs.iter().map(|surface| &surface.plugin))
            .chain(self.menus.iter().map(|surface| &surface.plugin))
            .chain(self.pages.iter().map(|surface| &surface.plugin))
        {
            if !found.contains(id) {
                found.push(id.clone());
            }
        }
        found
    }
}

/// One plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub vendor: String,
    pub description: String,
    /// What it does not do, which every one of these has and which is worth reading before wondering
    /// why a regular expression is coloured as division.
    pub limitations: String,
    pub kind: Kind,
    /// What this plugin adds to the window. Empty for every `language` plugin.
    pub contributions: Contributions,
    /// The extensions it claims, without the dot, in lower case.
    pub extensions: Vec<String>,
    /// The built-in renderer this language's files are drawn with, if it has one.
    pub renders: Option<String>,
    /// How one file of this language is run, with `{file}` standing for the path — `node {file}`.
    ///
    /// What puts `Run Current File` on the run widget's flyout and the `Run` menu for a file of
    /// this language, and nothing else. Off unless a manifest asks for it, which is the rule every
    /// key added since `task-1671` has followed.
    pub run_file: Option<String>,
    /// The project detector this language's projects are found by, named from [`PROJECT_RUNNERS`].
    pub run_project: Option<String>,
    /// The debugger this language's files are debugged with, named from [`DEBUGGERS`].
    ///
    /// What puts the whole debug half of the Run menu, the gutter's breakpoints and the debug tile
    /// in front of a file of this language, and nothing else. **Absent** rather than dimmed for a
    /// language that names none, which is the rule the three code-navigation entries already follow:
    /// a stylesheet has nothing to step through and never will.
    pub debug_adapter: Option<String>,
    pub grammar: Grammar,
    pub theme: SyntaxTheme,
    /// The bytes of `icon.png`, when it has one.
    pub icon: Option<Vec<u8>>,
    /// True when it came from inside the binary rather than from disk.
    pub bundled: bool,
    /// True when it is switched on.
    pub enabled: bool,
}

impl Plugin {
    /// Whether this plugin claims `path`.
    pub fn claims(&self, path: &Path) -> bool {
        let Some(extension) = path.extension().and_then(|name| name.to_str()) else {
            return false;
        };
        let extension = extension.to_lowercase();
        self.extensions.iter().any(|known| *known == extension)
    }
}

/// Everything installed, and which of them are switched on.
#[derive(Debug, Clone, Default)]
pub struct Plugins {
    installed: Vec<Plugin>,
}

impl Plugins {
    /// Read the bundled plugins, then anything on disk, which shadows a bundled one of the same id.
    ///
    /// A plugin that will not parse is skipped, and the reason is returned rather than thrown away.
    /// Quill starting with one plugin fewer is better than Quill refusing to start — the same rule
    /// `store.rs` already keeps for a settings file with a stray line in it.
    pub fn load(store: Option<&Store>) -> (Self, Vec<String>) {
        let mut installed: Vec<Plugin> = Vec::new();
        let mut problems: Vec<String> = Vec::new();
        for (id, manifest, icon) in bundled::ALL {
            match parse(&Values::parse(manifest), true) {
                Ok(mut plugin) => {
                    plugin.icon = icon.map(<[u8]>::to_vec);
                    installed.push(plugin);
                }
                Err(reason) => problems.push(format!("{id}: {reason}")),
            }
        }
        if let Some(store) = store {
            let folder = store.folder().join(FOLDER);
            if let Ok(entries) = std::fs::read_dir(&folder) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    match read_folder(&path) {
                        Ok(plugin) => {
                            // A plugin on disk shadows the bundled one of the same id, so a bundled
                            // one can be corrected by hand without rebuilding Quill.
                            installed.retain(|known| known.id != plugin.id);
                            installed.push(plugin);
                        }
                        Err(reason) => {
                            problems.push(format!("{}: {reason}", path.display()));
                        }
                    }
                }
            }
            let disabled = store.read_values();
            if let Some(list) = disabled.text("plugins.disabled") {
                for id in list.split(',').map(str::trim).filter(|id| !id.is_empty()) {
                    if let Some(plugin) = installed.iter_mut().find(|plugin| plugin.id == id) {
                        plugin.enabled = false;
                    }
                }
            }
        }
        installed.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        (Self { installed }, problems)
    }

    pub fn all(&self) -> &[Plugin] {
        &self.installed
    }

    pub fn get(&self, id: &str) -> Option<&Plugin> {
        self.installed.iter().find(|plugin| plugin.id == id)
    }

    /// Everything every plugin that is switched on contributes.
    ///
    /// Worked out from the manifests rather than remembered, so it is right after `set_enabled`,
    /// after `install` and after a reload, with nothing to invalidate. The plugins are already sorted
    /// by name, so the rail's buttons and the menus are in the same order every time.
    pub fn surfaces(&self) -> Surfaces {
        let mut surfaces = Surfaces::default();
        for plugin in self.installed.iter().filter(|plugin| plugin.enabled) {
            let Some(provider) = plugin.contributions.provider.clone() else {
                continue;
            };
            // One closure per list rather than one generic one, because a closure in Rust is not
            // generic over its argument and four contributions are four types.
            fn made<T>(plugin: &Plugin, provider: &str, what: T) -> Surface<T> {
                Surface { plugin: plugin.id.clone(), provider: provider.to_owned(), what }
            }
            if let Some(pane) = plugin.contributions.pane.clone() {
                surfaces.panes.push(made(plugin, &provider, pane));
            }
            if let Some(tab) = plugin.contributions.tab.clone() {
                surfaces.tabs.push(made(plugin, &provider, tab));
            }
            if let Some(menu) = plugin.contributions.menu.clone() {
                surfaces.menus.push(made(plugin, &provider, menu));
            }
            if let Some(page) = plugin.contributions.page.clone() {
                surfaces.pages.push(made(plugin, &provider, page));
            }
            if let Some(renderer) = plugin.contributions.chrome.clone() {
                surfaces.chrome.push((plugin.id.clone(), renderer));
            }
        }
        surfaces
    }

    /// The plugins that draw, whether or not they are switched on, for the Plugins page's own list.
    pub fn ui_plugins(&self) -> Vec<&Plugin> {
        self.installed.iter().filter(|plugin| plugin.kind == Kind::Ui).collect()
    }

    pub fn enabled_count(&self) -> usize {
        self.installed.iter().filter(|plugin| plugin.enabled).count()
    }

    /// The plugin that claims `path`, if one does and it is switched on.
    pub fn for_path(&self, path: &Path) -> Option<&Plugin> {
        self.installed.iter().find(|plugin| plugin.enabled && plugin.claims(path))
    }

    /// True when some plugin that is switched on asks for the built-in renderer called `name`.
    ///
    /// The window asks this before it draws a diagram anywhere — a `.mmd` file's preview, and every
    /// mermaid block in a Markdown document — so switching the plugin off withdraws the feature in
    /// the same frame rather than at the next restart.
    /// The plugin that reads a language named on a fence in a Markdown document.
    ///
    /// A fence says `rust`, `rs`, `js` or `TypeScript`, and all four are the same request. So the
    /// word is matched against the plugin's id, its name and every extension it claims, which is
    /// what makes ```` ```rs ```` and ```` ```rust ```` one question without Quill holding a table
    /// of aliases that a plugin somebody writes later could not add to.
    pub fn for_language(&self, name: &str) -> Option<&Plugin> {
        let wanted = name.trim().trim_start_matches('.').to_lowercase();
        if wanted.is_empty() {
            return None;
        }
        self.installed.iter().filter(|plugin| plugin.enabled).find(|plugin| {
            plugin.id == wanted
                || plugin.name.to_lowercase() == wanted
                || plugin.extensions.contains(&wanted)
        })
    }

    pub fn renders(&self, name: &str) -> bool {
        self.installed
            .iter()
            .any(|plugin| plugin.enabled && plugin.renders.as_deref() == Some(name))
    }

    /// How one file of `path`'s language is run, when a plugin that is switched on says.
    ///
    /// Asked at the moment of use, exactly as [`Plugins::renders`] is, so switching the JavaScript
    /// plugin off withdraws `Run Current File` from `.js` files in the same frame rather than at
    /// the next restart.
    pub fn run_file(&self, path: &Path) -> Option<&str> {
        self.for_path(path)?.run_file.as_deref()
    }

    /// The debugger `path`'s language names, when a plugin that is switched on names one.
    ///
    /// **The one question the menus, the title bar, the gutter and the command line all ask**, so
    /// none of them can disagree about whether a file can be debugged — which is the rule
    /// `file_kind::definitions_apply` set and the reason there is a function here rather than four
    /// readings of `for_path`. Asked at the moment of use, exactly as [`Plugins::renders`] is, so
    /// switching the Rust plugin off withdraws debugging from `.rs` files in the same frame rather
    /// than at the next restart.
    pub fn debugger_for(&self, path: &Path) -> Option<&str> {
        self.for_path(path)?.debug_adapter.as_deref()
    }

    /// The languages this debugger debugs, as the plugins that are switched on say.
    ///
    /// The other direction of [`Plugins::debugger_for`], and it exists for `quill-cli debug
    /// adapters`: an agent asking whether it can debug wants to know what `lldb` is *for* here,
    /// which is a question only the manifests can answer.
    pub fn languages_debugged_by(&self, adapter: &str) -> Vec<String> {
        self.installed
            .iter()
            .filter(|plugin| plugin.enabled && plugin.debug_adapter.as_deref() == Some(adapter))
            .map(|plugin| plugin.name.clone())
            .collect()
    }

    /// True when any plugin that is switched on names a debugger at all, which is what decides
    /// whether the debug tile can ever be reached in this project.
    pub fn any_debugger(&self) -> bool {
        self.installed.iter().any(|plugin| plugin.enabled && plugin.debug_adapter.is_some())
    }

    /// The project detectors the plugins that are switched on have asked for, each named once.
    ///
    /// JavaScript and TypeScript both say `npm`, and both being installed is not two projects, so
    /// the list is deduplicated here rather than in the detector.
    pub fn project_runners(&self) -> Vec<&str> {
        let mut runners: Vec<&str> = Vec::new();
        for plugin in self.installed.iter().filter(|plugin| plugin.enabled) {
            if let Some(runner) = plugin.run_project.as_deref() {
                if !runners.contains(&runner) {
                    runners.push(runner);
                }
            }
        }
        runners
    }

    /// Switch a plugin on or off, and remember it.
    pub fn set_enabled(&mut self, store: Option<&Store>, id: &str, on: bool) {
        if let Some(plugin) = self.installed.iter_mut().find(|plugin| plugin.id == id) {
            plugin.enabled = on;
        }
        let Some(store) = store else {
            return;
        };
        let disabled: Vec<&str> = self
            .installed
            .iter()
            .filter(|plugin| !plugin.enabled)
            .map(|plugin| plugin.id.as_str())
            .collect();
        let mut values = store.read_values();
        values.set("plugins.disabled", disabled.join(", "));
        store.write_values(&values);
    }

    /// Write a bundled plugin out to the settings folder and read it back from there.
    ///
    /// Reading it back from disk rather than simply marking it installed is the point: it is what
    /// proves the loader works on real files and not only on what was baked into the binary.
    pub fn install(&mut self, store: &Store, id: &str) -> std::io::Result<()> {
        let Some((_, manifest, icon)) = bundled::ALL.iter().find(|(known, _, _)| *known == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("there is no plugin called {id}"),
            ));
        };
        let folder = store.folder().join(FOLDER).join(id);
        std::fs::create_dir_all(&folder)?;
        std::fs::write(folder.join(MANIFEST), manifest)?;
        if let Some(icon) = icon {
            std::fs::write(folder.join(ICON), icon)?;
        }
        let plugin = read_folder(&folder)
            .map_err(|reason| std::io::Error::new(std::io::ErrorKind::InvalidData, reason))?;
        self.installed.retain(|known| known.id != plugin.id);
        self.installed.push(plugin);
        self.installed.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(())
    }

    /// Where a plugin's folder is, so the marketplace can say whether it is on disk.
    pub fn folder(store: &Store, id: &str) -> PathBuf {
        store.folder().join(FOLDER).join(id)
    }

    /// The grammars of the plugins that are switched on, in a form a worker thread can hold.
    ///
    /// Taken as a snapshot rather than borrowed, because the two threads that read a project —
    /// `services::symbol_index` and the reference mode of `services::text_search` — outlive the
    /// frame that started them, and a plugin switched off while one is running must not change what
    /// it is half way through answering.
    pub fn grammars(&self) -> Grammars {
        let mut by_extension: Vec<(String, Grammar)> = Vec::new();
        for plugin in self.installed.iter().filter(|plugin| plugin.enabled) {
            for extension in &plugin.extensions {
                if by_extension.iter().any(|(known, _)| known == extension) {
                    continue; // the first plugin that claims an extension is the one `for_path` gives
                }
                by_extension.push((extension.clone(), plugin.grammar.clone()));
            }
        }
        Grammars { by_extension }
    }
}

/// Which grammar reads which extension, taken from the plugins that are switched on.
///
/// A list rather than a map: five plugins claim a dozen extensions between them, and a linear walk
/// over a dozen short strings costs less than hashing one. It is `Clone` and holds nothing borrowed,
/// so a copy can be sent to a thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grammars {
    by_extension: Vec<(String, Grammar)>,
}

impl Grammars {
    /// A set built by hand, for a test that needs one without a plugin folder behind it.
    ///
    /// Extensions are written without the dot, as `for_path` compares them.
    pub fn of(by_extension: Vec<(String, Grammar)>) -> Self {
        Self { by_extension }
    }

    /// The grammar that reads this file, if a plugin that is switched on claims it.
    pub fn for_path(&self, path: &Path) -> Option<&Grammar> {
        let extension = path.extension().and_then(|name| name.to_str())?.to_lowercase();
        self.by_extension
            .iter()
            .find(|(known, _)| *known == extension)
            .map(|(_, grammar)| grammar)
    }

    /// True when this file's language has said enough for a definition to be found in it.
    ///
    /// What the index reads a file at all for, and — through `services::file_kind` — what decides
    /// whether the three symbol entries are on the menu for it.
    pub fn defines_symbols(&self, path: &Path) -> bool {
        self.for_path(path).is_some_and(Grammar::defines_symbols)
    }

    /// How many extensions are claimed, for a test and for `symbol cost`.
    pub fn len(&self) -> usize {
        self.by_extension.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_extension.is_empty()
    }
}

/// Read one plugin folder.
fn read_folder(folder: &Path) -> Result<Plugin, String> {
    let manifest = folder.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest)
        .map_err(|problem| format!("{MANIFEST} could not be read: {problem}"))?;
    let mut plugin = parse(&Values::parse(&text), false)?;
    plugin.icon = std::fs::read(folder.join(ICON)).ok();
    Ok(plugin)
}

/// Turn a manifest into a plugin.
pub fn parse(values: &Values, bundled: bool) -> Result<Plugin, String> {
    let id = values.text("plugin.id").ok_or("plugin.id is missing")?.to_owned();
    // Checked rather than assumed, so a manifest for something Quill cannot run is refused with a
    // message instead of loading as half a language.
    let kind = match values.text("plugin.kind").unwrap_or("language") {
        "language" => Kind::Language,
        "ui" => Kind::Ui,
        other => {
            return Err(format!(
                "plugin.kind is `{other}`, and this version of Quill runs `language` and `ui` plugins"
            ))
        }
    };
    let extensions: Vec<String> = list(values, "language.extensions")
        .into_iter()
        .map(|extension| extension.trim_start_matches('.').to_lowercase())
        .collect();
    // A language claiming no file type would never be used, which is what this has always said. A UI
    // plugin claims none by construction — Agent-Tasks is not a file type — so the check belongs to the
    // kind rather than to every manifest.
    if kind == Kind::Language && extensions.is_empty() {
        return Err("language.extensions is empty, so nothing would ever use this plugin".to_owned());
    }
    let contributions = contributions(values, kind)?;
    // Checked against what this version can actually draw, for the same reason `plugin.kind` is: a
    // manifest naming a picture Quill does not have should say so rather than load as a language
    // whose files silently never draw.
    let renders = match values.text("language.renders").map(str::trim).filter(|name| !name.is_empty())
    {
        Some(name) if RENDERERS.contains(&name) => Some(name.to_owned()),
        Some(other) => {
            return Err(format!(
                "language.renders is `{other}`, and this version of Quill draws {}",
                RENDERERS.join(", ")
            ))
        }
        None => None,
    };
    let name = values.text("plugin.name").unwrap_or(&id).to_owned();
    let grammar = Grammar {
        language: name.clone(),
        keywords: list(values, "language.keywords"),
        builtins: list(values, "language.builtins"),
        types: list(values, "language.types"),
        line_comment: values.text("language.line_comment").map(str::to_owned),
        block_comment: pair(values, "language.block_comment"),
        strings: values
            .text("language.strings")
            .unwrap_or("\", '")
            .split(',')
            .filter_map(|quote| quote.trim().chars().next())
            .collect(),
        escapes: values.flag("language.escapes").unwrap_or(true),
        operators: values.text("language.operators").unwrap_or_default().chars().collect(),
        numbers: values.flag("language.numbers").unwrap_or(true),
        // Comma separated single characters, the way `language.strings` names its quotes. Empty for
        // every language but CSS, where a hyphen is a letter.
        word_characters: values
            .text("language.word_characters")
            .unwrap_or_default()
            .split(',')
            .filter_map(|character| character.trim().chars().next())
            .collect(),
        hex_colors: values.flag("language.hex_colors").unwrap_or(false),
        // The two `task-1675` added, both off unless a language asks for them, which is the rule
        // every key added since `task-1671` has followed and which
        // `the_older_plugins_ask_for_none_of_what_the_symbols_added` keeps.
        definers: definers(values)?,
        brace_definitions: values.flag("language.brace_definitions").unwrap_or(false),
        // The nine `task-1680` added, and the same rule again: a plugin that names none of them
        // behaves exactly as it did before, which
        // `the_older_plugins_ask_for_none_of_what_the_imports_added` keeps.
        export_keyword: word(values, "language.export_keyword"),
        imports: import_style(values)?,
        import_keywords: list(values, "language.import_keywords"),
        import_extensions: list(values, "language.import_extensions")
            .into_iter()
            .map(|extension| match extension.starts_with('.') {
                true => extension,
                false => format!(".{extension}"),
            })
            .collect(),
        import_index: list(values, "language.import_index"),
        import_omit_extension: values.flag("language.import_omit_extension").unwrap_or(false),
        path_separator: word(values, "language.path_separator"),
        source_roots: list(values, "language.source_roots"),
        path_roots: path_roots(values)?,
        // The two `task-1694` added, and the same rule a sixth time: a language that names neither
        // is read by exactly the code that read it before, which
        // `the_older_plugins_ask_for_none_of_what_the_markup_added` keeps.
        markup: values.flag("language.markup").unwrap_or(false),
        raw_text: raw_text(values)?,
    };
    let colours: Vec<(Token, Color)> = Token::ALL
        .into_iter()
        .filter_map(|token| {
            let value = values.text(&format!("theme.{}", token.name()))?;
            colour(value).map(|colour| (token, colour))
        })
        .collect();
    Ok(Plugin {
        id,
        name,
        version: values.text("plugin.version").unwrap_or("1.0.0").to_owned(),
        vendor: values.text("plugin.vendor").unwrap_or("Quill").to_owned(),
        description: values.text("plugin.description").unwrap_or_default().to_owned(),
        limitations: values.text("plugin.limitations").unwrap_or_default().to_owned(),
        kind,
        contributions,
        extensions,
        renders,
        run_file: run_file(values)?,
        run_project: run_project(values)?,
        debug_adapter: debug_adapter(values)?,
        grammar,
        theme: SyntaxTheme {
            name: values.text("theme.name").unwrap_or("Dracula").to_owned(),
            colours,
        },
        icon: None,
        bundled,
        enabled: true,
    })
}

/// The `ui.`, `pane.`, `tab.`, `menu.` and `settings.` keys: what a plugin adds to the window.
///
/// Every one of them is refused with a sentence naming what was asked for and what this version has,
/// which is the rule `plugin.kind`, `language.renders`, `run.project` and `debug.adapter` already keep.
/// A `language` manifest that names none of these parses exactly as it did before, which
/// `the_older_plugins_ask_for_none_of_what_the_ui_added` keeps.
fn contributions(values: &Values, kind: Kind) -> Result<Contributions, String> {
    let provider = match word(values, "ui.provider") {
        Some(named) if UI_PROVIDERS.contains(&named.as_str()) => Some(named),
        Some(named) => {
            return Err(format!(
                "ui.provider is `{named}`, and this version of Quill has {}",
                UI_PROVIDERS.join(", ")
            ))
        }
        None if kind == Kind::Ui => {
            return Err("ui.provider is missing, and a ui plugin with no provider would draw nothing"
                .to_owned())
        }
        None => None,
    };
    let chrome = match word(values, "ui.chrome") {
        Some(named) if CHROME.contains(&named.as_str()) => Some(named),
        Some(named) => {
            return Err(format!(
                "ui.chrome is `{named}`, and this version of Quill has {}",
                CHROME.join(", ")
            ))
        }
        None => None,
    };
    // A renderer with nothing to draw with it. `ui.chrome` says *how* a plugin's own pane is decorated,
    // and a language plugin has no pane — so this is a line that would do nothing, silently, which is what
    // every refusal in this function exists to prevent.
    if kind != Kind::Ui && chrome.is_some() {
        return Err(
            "ui.chrome is set on a plugin that is not a `ui` plugin, so there is no pane for it to draw"
                .to_owned(),
        );
    }
    let found = Contributions {
        provider,
        chrome,
        pane: pane(values)?,
        tab: tab(values),
        menu: menu(values)?,
        page: page(values)?,
    };
    // A key that asks for something the manifest did not declare is a line that does nothing, and a line
    // that does nothing silently is what every refusal here exists to prevent.
    no_orphans(values, "pane.", found.pane.is_some(), "pane.id")?;
    no_orphans(values, "tab.", found.tab.is_some(), "tab.id")?;
    no_orphans(values, "settings.", found.page.is_some(), "settings.page")?;
    no_orphans(values, "menu.", found.menu.is_some(), "menu.name")?;
    // A plugin adding no button, no pane, no tab, no menu and no settings page has no way of being
    // reached, so it is refused rather than installed as a row in a list that does nothing.
    if kind == Kind::Ui && found.is_empty() {
        return Err(
            "a ui plugin contributes nothing, so there would be no way to reach it: name a pane, a tab, a menu or a settings page"
                .to_owned(),
        );
    }
    if kind == Kind::Language && !found.is_empty() {
        return Err("plugin.kind is `language` and the manifest contributes to the window, which only a `ui` plugin does"
            .to_owned());
    }
    // A language plugin that names a provider is refused too, even though it contributes nothing: it asked
    // for code that only a `ui` plugin runs, and loading it would leave the provider unreachable.
    if kind == Kind::Language && found.provider.is_some() {
        return Err("plugin.kind is `language` and it names a ui.provider, which only a `ui` plugin has"
            .to_owned());
    }
    // And a plugin that draws must not carry a language's keys. A manifest naming both was read as a UI
    // plugin and its grammar, its renderer, its runner and its debugger were all silently dropped, which is
    // the outcome every other check here exists to prevent.
    if kind == Kind::Ui {
        for named in [
            "language.extensions",
            "language.renders",
            "language.keywords",
            "language.line_comment",
            "language.definers",
            "language.imports",
            "run.file",
            "run.project",
            "debug.adapter",
            "theme.name",
        ] {
            if values.text(named).map(str::trim).is_some_and(|value| !value.is_empty()) {
                return Err(format!(
                    "plugin.kind is `ui` and the manifest sets {named}, which only a `language` plugin has"
                ));
            }
        }
    }
    Ok(found)
}

/// `pane.*`: the button in the rail, and the pane it opens.
///
/// Five of the six keys have a default, so a manifest asking for a pane writes two lines. The
/// defaults are the explorer's width and the terminal's height, because those are the two numbers the
/// window already uses for a column at the side and a strip along the bottom.
fn pane(values: &Values) -> Result<Option<PaneContribution>, String> {
    let Some(id) = word(values, "pane.id") else {
        return Ok(None);
    };
    let group = match values.text("pane.group").map(str::trim).unwrap_or("top") {
        "top" => RailGroup::Top,
        "bottom" => RailGroup::Bottom,
        other => return Err(format!("pane.group is `{other}`, and the rail has top and bottom")),
    };
    let named_side = values.text("pane.side").map(str::trim).unwrap_or("right");
    let side = crate::app::dock::Side::from_name(named_side).ok_or_else(|| {
        format!("pane.side is `{named_side}`, and a panel docks to left, right, top or bottom")
    })?;
    let icon = match word(values, "pane.icon") {
        Some(named) if PANE_ICONS.contains(&named.as_str()) => named,
        Some(named) => {
            return Err(format!(
                "pane.icon is `{named}`, and this version of Quill draws {}",
                PANE_ICONS.join(", ")
            ))
        }
        None => "board".to_owned(),
    };
    let applies = match word(values, "pane.applies") {
        Some(named) if PANE_CONDITIONS.contains(&named.as_str()) => named,
        Some(named) => {
            return Err(format!(
                "pane.applies is `{named}`, and this version of Quill knows {}",
                PANE_CONDITIONS.join(", ")
            ))
        }
        None => "always".to_owned(),
    };
    Ok(Some(PaneContribution {
        label: word(values, "pane.label")
            .or_else(|| word(values, "plugin.name"))
            .unwrap_or_else(|| id.clone()),
        id,
        icon,
        group,
        side,
        width: measurement(values, "pane.width", 320.0)?,
        height: measurement(values, "pane.height", 260.0)?,
        applies,
    }))
}

/// `tab.*`: a tab in the editing area, which is opened from a menu rather than by opening a file.
fn tab(values: &Values) -> Option<TabContribution> {
    let id = word(values, "tab.id")?;
    Some(TabContribution {
        label: word(values, "tab.label")
            .or_else(|| word(values, "plugin.name"))
            .unwrap_or_else(|| id.clone()),
        id,
    })
}

/// `menu.*`: the plugin's own menu, its entries, and any submenus nested inside it.
///
/// `menu.entries` is a comma list of `command=Name`, and a lone `-` is a separator.
/// `menu.submenu.<id>` names a submenu and `menu.submenu.<id>.entries` fills it, so
/// `menu.submenu.new.submenu.other` is a submenu inside a submenu and the reader is recursive.
fn menu(values: &Values) -> Result<Option<MenuContribution>, String> {
    let Some(name) = word(values, "menu.name") else {
        return Ok(None);
    };
    Ok(Some(MenuContribution { name, items: menu_items(values, "menu")? }))
}

/// The entries under one `menu.` or `menu.submenu.<id>.` prefix, and the submenus under it.
fn menu_items(values: &Values, prefix: &str) -> Result<Vec<MenuItem>, String> {
    let mut items = Vec::new();
    for entry in list(values, &format!("{prefix}.entries")) {
        if entry == "-" {
            items.push(MenuItem::Separator);
            continue;
        }
        let Some((command, label)) = entry.split_once('=') else {
            return Err(format!(
                "{prefix}.entries holds `{entry}`, which is not `command=Name`"
            ));
        };
        let command = command.trim();
        let label = label.trim();
        if command.is_empty() || label.is_empty() {
            return Err(format!("{prefix}.entries holds `{entry}`, which is not `command=Name`"));
        }
        items.push(MenuItem::Command { command: command.to_owned(), label: label.to_owned() });
    }
    // The submenus, in the order the manifest names them, which `Values` keeps sorted so that a menu
    // is the same shape every time it is read.
    for (key, label) in values.starting_with(&format!("{prefix}.submenu.")) {
        // `menu.submenu.new` names one; `menu.submenu.new.entries` fills it and is not a name.
        if key.contains('.') {
            continue;
        }
        let label = label.trim();
        if label.is_empty() {
            return Err(format!("{prefix}.submenu.{key} has no name"));
        }
        let nested = menu_items(values, &format!("{prefix}.submenu.{key}"))?;
        if nested.is_empty() {
            return Err(format!(
                "{prefix}.submenu.{key} is empty, so it would open onto nothing"
            ));
        }
        items.push(MenuItem::Submenu { label: label.to_owned(), items: nested });
    }
    Ok(items)
}

/// `settings.*`: the plugin's page in the Settings window.
fn page(values: &Values) -> Result<Option<PageContribution>, String> {
    let Some(name) = word(values, "settings.page") else {
        return Ok(None);
    };
    let icon = match word(values, "settings.icon") {
        Some(named) if PANE_ICONS.contains(&named.as_str()) => named,
        Some(named) => {
            return Err(format!(
                "settings.icon is `{named}`, and this version of Quill draws {}",
                PANE_ICONS.join(", ")
            ))
        }
        None => "board".to_owned(),
    };
    Ok(Some(PageContribution { name, icon }))
}

/// A number from the manifest, with the default when it is absent and a refusal when it is not a
/// number, because a width of `wide` silently becoming 320 is the outcome every check here prevents.
fn measurement(values: &Values, name: &str, default: f32) -> Result<f32, String> {
    match values.text(name).map(str::trim).filter(|text| !text.is_empty()) {
        // A width of `wide` silently becoming 320 is the outcome every check here exists to prevent, so it
        // is refused with what it said and what a width is.
        Some(text) => match text.parse::<f32>() {
            Ok(number) if number > 0.0 => Ok(number),
            _ => Err(format!("{name} is `{text}`, and a measurement is a number of points above zero")),
        },
        None => Ok(default),
    }
}

/// Refuse a `pane.`, `tab.` or `settings.` key on a manifest that asks for no such contribution.
///
/// `pane.width` with no `pane.id` is a line somebody wrote expecting it to do something, and it does
/// nothing. Saying so is the difference between a manifest that is wrong and a manifest that is wrong and
/// silent about it.
fn no_orphans(values: &Values, prefix: &str, present: bool, needs: &str) -> Result<(), String> {
    if present {
        return Ok(());
    }
    let orphans = values.starting_with(prefix);
    match orphans.first() {
        Some((rest, _)) => Err(format!(
            "the manifest sets {prefix}{rest} and has no {needs}, so nothing would read it"
        )),
        None => Ok(()),
    }
}

/// `run.file`: the command that runs one file of this language, with `{file}` for the path.
///
/// The placeholder is **required**, because a template without it would run the same file whatever
/// tab was open — a manifest that appears to work and quietly does the wrong thing, which is the
/// one outcome every other check in this file exists to prevent. Nothing else about it is checked:
/// it is a command line, and `services::run_configurations::split_command` reads it the way it
/// reads every other one.
fn run_file(values: &Values) -> Result<Option<String>, String> {
    let Some(template) = word(values, "run.file") else {
        return Ok(None);
    };
    if !template.contains(crate::services::run_configurations::FILE_PLACEHOLDER) {
        return Err(format!(
            "run.file is `{template}`, which has no {} in it, so it would run the same file whatever was open",
            crate::services::run_configurations::FILE_PLACEHOLDER
        ));
    }
    Ok(Some(template))
}

/// `run.project`: the name of a project detector built into Quill.
///
/// Checked against [`PROJECT_RUNNERS`] exactly as `language.renders` is checked against
/// [`RENDERERS`]. **Nothing in a plugin is executed**: the manifest says "a project of this
/// language announces itself, and this is which detector notices", and the code that reads
/// `Cargo.toml` and `package.json` shipped with the binary. The most a third-party manifest can do
/// is suggest text, visibly.
fn run_project(values: &Values) -> Result<Option<String>, String> {
    let Some(named) = word(values, "run.project") else {
        return Ok(None);
    };
    if PROJECT_RUNNERS.contains(&named.as_str()) {
        return Ok(Some(named));
    }
    Err(format!(
        "run.project is `{named}`, and this version of Quill detects {}",
        PROJECT_RUNNERS.join(", ")
    ))
}

/// `debug.adapter`: the name of a debugger built into Quill.
///
/// Checked against [`DEBUGGERS`] exactly as `run.project` is checked against [`PROJECT_RUNNERS`],
/// and the refusal reads the same way, because it is the same decision made a third time: the
/// manifest says "files of this language can be debugged, and this is which debugger knows how", and
/// the code that finds and drives that debugger shipped with the binary.
fn debug_adapter(values: &Values) -> Result<Option<String>, String> {
    let Some(named) = word(values, "debug.adapter") else {
        return Ok(None);
    };
    if DEBUGGERS.contains(&named.as_str()) {
        return Ok(Some(named));
    }
    Err(format!(
        "debug.adapter is `{named}`, and this version of Quill drives {}",
        DEBUGGERS.join(", ")
    ))
}

/// `language.definers`: a comma list of `keyword=kind` saying which keyword makes the word after
/// it a definition, and of what.
///
/// The kind is checked against what `quill_core::symbols` actually has, for the same reason
/// `plugin.kind` and `language.renders` are checked: a manifest asking for something this version
/// does not know should say so plainly rather than load as a language whose declarations are
/// quietly never found. An entry that is not a pair is refused for the same reason — silently
/// dropping it would leave a language half able to answer.
fn definers(values: &Values) -> Result<Vec<(String, SymbolKind)>, String> {
    let mut found = Vec::new();
    for entry in list(values, "language.definers") {
        let Some((keyword, kind)) = entry.split_once('=') else {
            return Err(format!("language.definers holds `{entry}`, which is not `keyword=kind`"));
        };
        let Some(parsed) = SymbolKind::parse(kind) else {
            let known: Vec<&str> = SymbolKind::ALL.iter().map(|kind| kind.name()).collect();
            return Err(format!(
                "language.definers says `{entry}`, and a definition in Quill is one of {}",
                known.join(", ")
            ));
        };
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Err(format!("language.definers holds `{entry}`, which names no keyword"));
        }
        found.push((keyword.to_owned(), parsed));
    }
    Ok(found)
}

/// `language.imports`: which of the two shapes of import this language writes.
///
/// Checked against what this version can actually read, for the same reason `plugin.kind`,
/// `language.renders` and `language.definers` are: a manifest asking for a third shape should say
/// so plainly rather than load as a language whose imports quietly never complete.
fn import_style(values: &Values) -> Result<Option<ImportStyle>, String> {
    let Some(named) = word(values, "language.imports") else {
        return Ok(None);
    };
    match ImportStyle::parse(&named) {
        Some(style) => Ok(Some(style)),
        None => {
            let known: Vec<&str> = ImportStyle::ALL.iter().map(|style| style.name()).collect();
            Err(format!(
                "language.imports is `{named}`, and an import in Quill is written {}",
                known.join(" or ")
            ))
        }
    }
}

/// `language.path_roots`: a comma list of `word=meaning` naming the segments of a module path that
/// are not module names — `crate=package, self=module, super=parent`.
fn path_roots(values: &Values) -> Result<Vec<(String, PathRoot)>, String> {
    let mut found = Vec::new();
    for entry in list(values, "language.path_roots") {
        let Some((word, meaning)) = entry.split_once('=') else {
            return Err(format!("language.path_roots holds `{entry}`, which is not `word=meaning`"));
        };
        let Some(parsed) = PathRoot::parse(meaning) else {
            let known: Vec<&str> = PathRoot::ALL.iter().map(|root| root.name()).collect();
            return Err(format!(
                "language.path_roots says `{entry}`, and a root in Quill is one of {}",
                known.join(", ")
            ));
        };
        let word = word.trim();
        if word.is_empty() {
            return Err(format!("language.path_roots holds `{entry}`, which names no word"));
        }
        found.push((word.to_owned(), parsed));
    }
    Ok(found)
}

/// `language.raw_text`: a comma list of `element` or `element=language`, the elements of a markup
/// language whose contents are not markup — `script=javascript, style=css, textarea, title`.
///
/// The right hand side is a language name and it is **not** checked here, which is the one
/// registry-shaped key in Quill that is not validated against a list: the name is resolved by
/// `Plugins::for_language` at the moment of use, the same function a fence in a Markdown document
/// is resolved by, which already answers with nothing for a language nothing claims. Checking it
/// would mean a plugin refusing to load because another plugin was switched off. An entry that
/// names a language is a raw text element and one that names none is an escapable raw text one,
/// which is the HTML Standard's own distinction and is derived rather than written down twice.
fn raw_text(values: &Values) -> Result<Vec<(String, Option<String>)>, String> {
    let mut found = Vec::new();
    for entry in list(values, "language.raw_text") {
        let (element, language) = match entry.split_once('=') {
            Some((element, language)) => (element, Some(language)),
            None => (entry.as_str(), None),
        };
        let element = element.trim();
        if element.is_empty() {
            return Err(format!("language.raw_text holds `{entry}`, which names no element"));
        }
        let language = language.map(str::trim).filter(|language| !language.is_empty());
        if entry.contains('=') && language.is_none() {
            return Err(format!("language.raw_text holds `{entry}`, which names an element and no language"));
        }
        found.push((element.to_owned(), language.map(str::to_owned)));
    }
    Ok(found)
}

/// One trimmed word, or nothing when the manifest left the key out or left it empty.
fn word(values: &Values, name: &str) -> Option<String> {
    values.text(name).map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

/// A comma separated value as a list, with the spaces trimmed and the empty entries dropped.
fn list(values: &Values, name: &str) -> Vec<String> {
    values
        .text(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Two comma separated values, which is what a block comment's opener and terminator are.
fn pair(values: &Values, name: &str) -> Option<(String, String)> {
    let parts = list(values, name);
    match parts.as_slice() {
        [open, close] => Some((open.clone(), close.clone())),
        _ => None,
    }
}

/// `#RRGGBB`, or `RRGGBB`.
pub fn colour(text: &str) -> Option<Color> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// The plugins that ship inside the binary.
///
/// They are bundled so that a Quill that has just been installed colours a `.rs` file the first time
/// it opens one, and so that the marketplace has something in it with no network involved.
pub mod bundled {
    /// Each entry is an id, its manifest, and its icon.
    pub const ALL: &[(&str, &str, Option<&[u8]>)] = &[
        (
            "javascript",
            include_str!("../../plugins/javascript/plugin.conf"),
            Some(include_bytes!("../../plugins/javascript/icon.png")),
        ),
        (
            "typescript",
            include_str!("../../plugins/typescript/plugin.conf"),
            Some(include_bytes!("../../plugins/typescript/icon.png")),
        ),
        (
            "rust",
            include_str!("../../plugins/rust/plugin.conf"),
            Some(include_bytes!("../../plugins/rust/icon.png")),
        ),
        (
            "css",
            include_str!("../../plugins/css/plugin.conf"),
            Some(include_bytes!("../../plugins/css/icon.png")),
        ),
        // The first plugin that draws. It has no icon of its own: its rail button is `pane.icon`,
        // which is a drawn icon rather than a picture, so it follows the pointer's opacity and the
        // window's colours the way every other button in the rail does.
        ("agent-tasks", include_str!("../../plugins/agent-tasks/plugin.conf"), None),
        (
            "mermaid",
            include_str!("../../plugins/mermaid/plugin.conf"),
            Some(include_bytes!("../../plugins/mermaid/icon.png")),
        ),
        (
            "html",
            include_str!("../../plugins/html/plugin.conf"),
            Some(include_bytes!("../../plugins/html/icon.png")),
        ),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> String {
        [
            "plugin.id = sample",
            "plugin.name = Sample",
            "plugin.version = 2.1.0",
            "plugin.vendor = Someone",
            "plugin.description = A sample.",
            "language.extensions = .smp, SMPL",
            "language.keywords = if, else, while",
            "language.builtins = print",
            "language.line_comment = //",
            "language.block_comment = /*, */",
            "language.strings = \", '",
            "language.operators = +-=",
            "theme.keyword = #FF79C6",
            "theme.comment = 6272A4",
            "theme.number = not a colour",
        ]
        .join("\n")
    }

    #[test]
    fn a_manifest_becomes_a_plugin() {
        let plugin = parse(&Values::parse(&manifest()), false).expect("it should parse");
        assert_eq!(plugin.id, "sample");
        assert_eq!(plugin.name, "Sample");
        assert_eq!(plugin.version, "2.1.0");
        assert_eq!(plugin.vendor, "Someone");
        assert_eq!(plugin.kind, Kind::Language);
        // The dot is optional and the case does not matter, because a person writing a manifest
        // should not have to know which Quill wanted.
        assert_eq!(plugin.extensions, vec!["smp", "smpl"]);
        assert_eq!(plugin.grammar.keywords, vec!["if", "else", "while"]);
        assert_eq!(plugin.grammar.block_comment, Some(("/*".to_owned(), "*/".to_owned())));
        assert_eq!(plugin.grammar.strings, vec!['"', '\'']);
    }

    #[test]
    fn a_colour_is_read_with_or_without_its_hash_and_a_bad_one_is_left_out() {
        let plugin = parse(&Values::parse(&manifest()), false).expect("it should parse");
        assert_eq!(plugin.theme.colour(Token::Keyword), Some(Color::rgb(0xFF, 0x79, 0xC6)));
        assert_eq!(plugin.theme.colour(Token::Comment), Some(Color::rgb(0x62, 0x72, 0xA4)));
        assert_eq!(plugin.theme.colour(Token::Number), None, "a value that is not a colour is skipped");
        assert_eq!(plugin.theme.colour(Token::String), None, "a colour that was not named is absent");
    }

    #[test]
    fn a_manifest_with_no_id_or_no_extensions_is_refused_with_a_reason() {
        let problem = parse(&Values::parse("language.extensions = .a"), false).expect_err("no id");
        assert!(problem.contains("plugin.id"));
        let problem = parse(&Values::parse("plugin.id = a"), false).expect_err("no extensions");
        assert!(problem.contains("extensions"));
    }

    #[test]
    fn a_kind_this_version_cannot_run_is_refused_rather_than_half_loaded() {
        // The seam a later version widens. A manifest asking for something Quill cannot do must say
        // so plainly rather than loading as a language with no grammar.
        let text = "plugin.id = a\nplugin.kind = wasm\nlanguage.extensions = .a";
        let problem = parse(&Values::parse(text), false).expect_err("wasm is not a kind yet");
        assert!(problem.contains("wasm") && problem.contains("language"), "{problem}");
    }

    #[test]
    fn a_plugin_claims_its_own_extensions_and_no_others() {
        let plugin = parse(&Values::parse(&manifest()), false).expect("it should parse");
        assert!(plugin.claims(Path::new("thing.smp")));
        assert!(plugin.claims(Path::new("thing.SMP")), "the extension check ignores case");
        assert!(plugin.claims(Path::new("thing.smpl")));
        assert!(!plugin.claims(Path::new("thing.rs")));
        assert!(!plugin.claims(Path::new("thing")));
    }

    /// A manifest for a plugin that draws, with every contribution on it. Used by the tests below and
    /// by the surfaces tests, so that a change to the keys is a change in one place.
    fn ui_manifest() -> String {
        [
            "plugin.id = board-plugin",
            "plugin.name = Board",
            "plugin.kind = ui",
            "ui.provider = agent-tasks",
            "pane.id = board",
            "pane.icon = board",
            "pane.side = right",
            "pane.width = 420",
            "tab.id = board",
            "menu.name = Board",
            "menu.entries = open=Open Board, -, sync=Sync",
            "menu.submenu.new = New",
            "menu.submenu.new.entries = task=Task, epic=Epic",
            "settings.page = Board",
        ]
        .join("\n")
    }

    #[test]
    fn a_ui_plugin_reads_all_five_contributions_and_needs_no_file_type() {
        let plugin = parse(&Values::parse(&ui_manifest()), false).expect("a ui manifest");
        assert_eq!(plugin.kind, Kind::Ui);
        // The check that refuses a language with no extensions belongs to the kind: Agent-Tasks is not
        // a file type and never will be.
        assert!(plugin.extensions.is_empty());
        let contributed = &plugin.contributions;
        assert_eq!(contributed.provider.as_deref(), Some("agent-tasks"));
        let pane = contributed.pane.as_ref().expect("a pane");
        assert_eq!(pane.id, "board");
        assert_eq!(pane.label, "Board", "the label falls back to plugin.name");
        assert_eq!(pane.group, RailGroup::Top, "top is the default");
        assert_eq!(pane.side, crate::app::dock::Side::Right);
        assert_eq!(pane.width, 420.0);
        assert_eq!(pane.height, 260.0, "the terminal's height is the default for a strip");
        assert_eq!(pane.applies, "always");
        assert_eq!(contributed.tab.as_ref().expect("a tab").label, "Board");
        assert_eq!(contributed.page.as_ref().expect("a page").name, "Board");
        let menu = contributed.menu.as_ref().expect("a menu");
        assert_eq!(menu.name, "Board");
        assert_eq!(
            menu.items,
            vec![
                MenuItem::Command { command: "open".to_owned(), label: "Open Board".to_owned() },
                MenuItem::Separator,
                MenuItem::Command { command: "sync".to_owned(), label: "Sync".to_owned() },
                MenuItem::Submenu {
                    label: "New".to_owned(),
                    items: vec![
                        MenuItem::Command { command: "task".to_owned(), label: "Task".to_owned() },
                        MenuItem::Command { command: "epic".to_owned(), label: "Epic".to_owned() },
                    ],
                },
            ],
            "the entries are in the order the manifest names them, and a submenu comes after them"
        );
    }

    #[test]
    fn a_submenu_inside_a_submenu_is_read() {
        let text = [
            "plugin.id = a",
            "plugin.kind = ui",
            "ui.provider = agent-tasks",
            "menu.name = A",
            "menu.submenu.new = New",
            "menu.submenu.new.entries = task=Task",
            "menu.submenu.new.submenu.from = From",
            "menu.submenu.new.submenu.from.entries = jira=JIRA",
        ]
        .join("\n");
        let plugin = parse(&Values::parse(&text), false).expect("nested submenus");
        let menu = plugin.contributions.menu.expect("a menu");
        let MenuItem::Submenu { label, items } = &menu.items[0] else {
            panic!("the first item should be the New submenu, got {:?}", menu.items[0]);
        };
        assert_eq!(label, "New");
        assert_eq!(items.len(), 2, "one command and one submenu inside it");
        assert!(matches!(items[1], MenuItem::Submenu { .. }), "{:?}", items[1]);
    }

    #[test]
    fn every_refusal_names_what_was_asked_for_and_what_this_version_has() {
        // The rule `plugin.kind`, `language.renders`, `run.project` and `debug.adapter` already keep,
        // made once for each new key. A message that only says `invalid` sends somebody to the source.
        let cases: &[(&str, &[&str])] = &[
            ("plugin.id = a\nplugin.kind = wasm", &["wasm", "language", "ui"]),
            ("plugin.id = a\nplugin.kind = ui", &["ui.provider is missing"]),
            ("plugin.id = a\nplugin.kind = ui\nui.provider = chat", &["chat", "agent-tasks"]),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks",
                &["contributes nothing", "pane", "tab", "menu", "settings page"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.group = middle",
                &["middle", "top", "bottom"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.side = middle",
                &["middle", "left", "right", "top", "bottom"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.icon = sparkle",
                &["sparkle", "board", "terminal"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.applies = has_git",
                &["has_git", "always", "in_project"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\nmenu.name = A\nmenu.entries = open",
                &["open", "command=Name"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\nmenu.name = A\nmenu.submenu.new = New",
                &["menu.submenu.new", "empty"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\nsettings.page = A\nsettings.icon = sparkle",
                &["sparkle", "board"],
            ),
            // A measurement that is not a measurement. Silently becoming the default is the outcome every
            // check here exists to prevent.
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.width = wide",
                &["pane.width", "wide", "number of points"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\npane.height = 0",
                &["pane.height", "above zero"],
            ),
            // A key that asks for a contribution the manifest did not declare.
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\nmenu.name = A\nmenu.entries = x=X\npane.width = 400",
                &["pane.width", "pane.id", "nothing would read it"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\ntab.label = Board",
                &["tab.label", "tab.id"],
            ),
            // A plugin that draws must not carry a language's keys, and a language must not name a provider.
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\nlanguage.extensions = .a",
                &["language.extensions", "only a `language` plugin"],
            ),
            (
                "plugin.id = a\nplugin.kind = ui\nui.provider = agent-tasks\npane.id = b\ndebug.adapter = lldb",
                &["debug.adapter", "only a `language` plugin"],
            ),
            (
                "plugin.id = a\nlanguage.extensions = .a\nui.provider = agent-tasks",
                &["ui.provider", "only a `ui` plugin"],
            ),
            // Unchanged: a language with no file type is still refused, and for the same reason.
            ("plugin.id = a\nlanguage.extensions =", &["language.extensions is empty"]),
            // A language manifest that contributes to the window is refused rather than half-loaded,
            // because a pane drawn by a plugin with no provider would be a pane nothing fills.
            (
                "plugin.id = a\nlanguage.extensions = .a\npane.id = b",
                &["language", "only a `ui` plugin"],
            ),
        ];
        for (text, expected) in cases {
            let problem = parse(&Values::parse(text), false)
                .expect_err(&format!("this should be refused:\n{text}"));
            for word in *expected {
                assert!(problem.contains(word), "`{word}` is not in `{problem}`");
            }
        }
    }

    #[test]
    fn the_older_plugins_ask_for_none_of_what_the_ui_added() {
        // The rule every round of keys has kept since `task-1671`: a language that names none of the
        // new keys is read by exactly the code that read it before. This is what proves the reader's
        // change has not moved Rust, CSS, HTML, JavaScript, TypeScript or Mermaid.
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        for plugin in plugins.all().iter().filter(|plugin| plugin.kind == Kind::Language) {
            assert_eq!(
                plugin.contributions,
                Contributions::default(),
                "{} contributes to the window and should not",
                plugin.id
            );
        }
        let mermaid = plugins.get("mermaid").expect("the mermaid plugin");
        assert_eq!(mermaid.kind, Kind::Language, "Mermaid is a language, not a plugin that draws");
    }

    /// `task-28` asked for the board to be a tab and not a pane, so it contributes four things and one of
    /// them is the provider. A pane is still something a manifest may ask for — `a_manifest_may_contribute_a_pane`
    /// below is what keeps the reader honest about that, since no bundled plugin asks for one any more.
    #[test]
    fn the_agent_tasks_plugin_contributes_a_tab_a_menu_and_a_page_and_no_pane() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let board = plugins.get("agent-tasks").expect("the agent-tasks plugin");
        assert_eq!(board.kind, Kind::Ui);
        assert_eq!(board.contributions.provider.as_deref(), Some("agent-tasks"));
        assert!(board.contributions.pane.is_none(), "the board is a tab and nothing else");
        assert!(board.contributions.tab.is_some());
        assert!(board.contributions.menu.is_some());
        assert!(board.contributions.page.is_some());
        assert!(!board.limitations.is_empty(), "it says what it does not do");
        assert!(
            board.limitations.contains("no pane"),
            "and it says the board has no pane, since that is a control somebody may look for: {}",
            board.limitations
        );
    }

    /// `task-1765`: the board asks for the decoration renderer, and the key is checked like every other one.
    #[test]
    fn the_agent_tasks_plugin_asks_for_the_decoration_renderer_and_a_language_plugin_does_not() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let board = plugins.get("agent-tasks").expect("the agent-tasks plugin");
        assert_eq!(board.contributions.chrome.as_deref(), Some("vello"));
        // And it is in the one value everything reads, so switching the plugin off withdraws the decoration
        // in the same frame it withdraws the tab. The **name** is what comes back, not a yes: there is one
        // renderer today and the day there are two this is where the second one is chosen.
        assert_eq!(plugins.surfaces().chrome_for("agent-tasks"), Some("vello"));
        assert_eq!(plugins.surfaces().chrome_for("mermaid"), None);
        for plugin in plugins.all().iter().filter(|plugin| plugin.kind == Kind::Language) {
            assert!(plugin.contributions.chrome.is_none(), "{} asks for a renderer", plugin.id);
        }
    }

    #[test]
    fn a_renderer_this_version_does_not_have_is_refused_with_the_list_of_the_ones_it_does() {
        // The rule `plugin.kind`, `language.renders`, `run.project` and `debug.adapter` all keep: a manifest
        // naming something Quill has not got says so plainly rather than loading as a plugin whose pane is
        // quietly flat, which is the exact outcome a checked registry exists to prevent.
        let manifest = "plugin.id = a-board\nplugin.name = A Board\nplugin.kind = ui\n\
                        ui.provider = agent-tasks\nui.chrome = crayons\ntab.id = board\ntab.label = A Board\n";
        let problem = parse(&Values::parse(manifest), false).expect_err("crayons is not a renderer");
        assert!(problem.contains("ui.chrome is `crayons`"), "{problem}");
        assert!(problem.contains("vello"), "and it names what this version does have: {problem}");

        // And a renderer on a plugin with no pane to draw is refused too, rather than parsing into a line
        // that does nothing. A language plugin has no surface of its own.
        let language = "plugin.id = a-language
plugin.name = A Language
language.extensions = .aa
                        ui.chrome = vello
";
        let problem = parse(&Values::parse(language), false).expect_err("a language has no pane");
        assert!(problem.contains("not a `ui` plugin"), "{problem}");
    }

    /// A manifest that asks for a pane still gets one, read out of the file the way every other key is.
    ///
    /// This used to be covered by Agent-Tasks\'s own manifest and stopped being when `task-28` removed its
    /// pane. Reading a pane is Quill\'s side of the plugin contract rather than one plugin\'s arrangement, so
    /// it is tested against a manifest written here.
    #[test]
    fn a_manifest_may_contribute_a_pane() {
        let manifest = "plugin.id = a-board\nplugin.name = A Board\nplugin.kind = ui\n\
                        ui.provider = agent-tasks\npane.id = board\npane.label = A Board\n\
                        pane.icon = board\npane.group = top\npane.side = right\npane.width = 420\n";
        let plugin = parse(&Values::parse(manifest), false).expect("a manifest with a pane in it");
        let pane = plugin.contributions.pane.as_ref().expect("a pane");
        assert_eq!(pane.id, "board");
        assert_eq!(pane.label, "A Board");
        assert_eq!(pane.side, crate::app::dock::Side::Right);
        assert_eq!(pane.group, RailGroup::Top);
        assert_eq!(pane.width, 420.0);
    }

    #[test]
    fn the_surfaces_are_what_the_enabled_plugins_contribute_and_nothing_else() {
        let (mut plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let surfaces = plugins.surfaces();
        // No bundled plugin contributes a pane since `task-28`, so nothing is in a dock slot and the rail has
        // no contributed button.
        assert!(surfaces.panes.is_empty(), "the one plugin that draws contributes a tab, not a pane");
        assert!(surfaces.pane("agent-tasks/board").is_none());
        assert!(surfaces.pane("agent-tasks/nothing").is_none());
        assert_eq!(surfaces.tabs.len(), 1, "one plugin draws today");
        assert_eq!(surfaces.tabs[0].plugin, "agent-tasks");
        assert_eq!(surfaces.tabs[0].provider, "agent-tasks");
        assert_eq!(surfaces.tabs[0].key("board"), "agent-tasks/board");
        assert!(surfaces.tab("agent-tasks/board").is_some());
        assert_eq!(surfaces.menus.len(), 1);
        assert_eq!(surfaces.pages.len(), 1);
        // Switching it off withdraws every contribution at once, which is the rule `Plugins::renders`
        // already keeps for a Mermaid diagram: the window asks before it draws.
        plugins.set_enabled(None, "agent-tasks", false);
        assert!(plugins.surfaces().is_empty(), "a plugin that is off contributes nothing");
        plugins.set_enabled(None, "agent-tasks", true);
        assert_eq!(plugins.surfaces().tabs.len(), 1, "and switching it back on is one frame too");
    }

    #[test]
    fn the_bundled_plugins_all_parse_and_claim_what_they_should() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "a bundled plugin should always parse: {problems:?}");
        let ids: Vec<&str> = plugins.all().iter().map(|plugin| plugin.id.as_str()).collect();
        assert!(ids.contains(&"javascript") && ids.contains(&"typescript") && ids.contains(&"rust"));
        assert!(ids.contains(&"mermaid") && ids.contains(&"css") && ids.contains(&"html"));
        assert_eq!(plugins.for_path(Path::new("a.rs")).map(|p| p.id.as_str()), Some("rust"));
        assert_eq!(plugins.for_path(Path::new("a.ts")).map(|p| p.id.as_str()), Some("typescript"));
        assert_eq!(plugins.for_path(Path::new("a.js")).map(|p| p.id.as_str()), Some("javascript"));
        assert_eq!(plugins.for_path(Path::new("a.html")).map(|p| p.id.as_str()), Some("html"));
        assert_eq!(plugins.for_path(Path::new("a.htm")).map(|p| p.id.as_str()), Some("html"));
        assert_eq!(plugins.for_path(Path::new("a.md")), None, "Markdown is not a plugin's business");
        // Every language ships a file icon and a colour scheme, which is what the ticket asked for.
        // A plugin that draws ships neither: it has no files to put an icon in front of, and a colour
        // scheme it chose would be the one thing a plugin is never allowed to decide. Its rail button
        // is `pane.icon`, a drawn icon rather than a picture, so it follows the window's colours.
        for plugin in plugins.all() {
            assert!(!plugin.description.is_empty(), "{} says nothing about itself", plugin.id);
            match plugin.kind {
                Kind::Language => {
                    assert!(plugin.icon.is_some(), "{} has no icon", plugin.id);
                    assert!(!plugin.theme.is_empty(), "{} has no colour scheme", plugin.id);
                }
                Kind::Ui => {
                    assert!(plugin.theme.is_empty(), "{} names colours, which no plugin may", plugin.id);
                    assert!(plugin.extensions.is_empty(), "{} claims a file type", plugin.id);
                }
            }
        }
    }

    #[test]
    fn the_css_plugin_reads_the_three_things_a_stylesheet_needs() {
        // `task-1671`. Each of the three is off unless a manifest asks for it, so this is also what
        // proves they reach the grammar at all rather than being read and dropped.
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let css = plugins.get("css").expect("the css plugin");
        assert!(css.claims(Path::new("site.css")));
        assert!(css.claims(Path::new("SITE.CSS")));
        assert!(!css.claims(Path::new("site.scss")), "Sass is a different language, deliberately");
        assert_eq!(css.grammar.word_characters, vec!['-', '@'], "a hyphen is a letter in CSS");
        assert!(css.grammar.hex_colors, "and #ff0000 is a number");
        assert!(css.grammar.line_comment.is_none(), "// is not a comment in CSS");
        assert!(css.grammar.types.contains(&"flex".to_owned()), "the third list is read");
        assert!(css.grammar.builtins.contains(&"background-color".to_owned()));
        assert!(css.grammar.keywords.contains(&"@media".to_owned()));
        assert_eq!(css.renders, None, "a stylesheet has no picture");

        // What that adds up to, read through the tokeniser the window uses.
        use quill_core::syntax::{highlight, Token};
        let text = "@media screen { .card { background-color: #ff79c6; display: flex; } }";
        let found: Vec<(&str, Token)> = highlight(text, &css.grammar)
            .into_iter()
            .map(|(range, token)| (&text[range], token))
            .collect();
        assert!(found.contains(&("@media", Token::Keyword)), "{found:?}");
        assert!(found.contains(&("background-color", Token::Builtin)), "{found:?}");
        assert!(found.contains(&("#ff79c6", Token::Number)), "{found:?}");
        assert!(found.contains(&("flex", Token::Type)), "{found:?}");
    }

    #[test]
    fn the_older_plugins_ask_for_none_of_what_css_added() {
        // The three keys are opt-in, which is what keeps a `.ts` file coloured exactly as it was.
        // HTML is the one plugin besides CSS to read a hyphen as a letter, so it is the one the
        // loop lets through; `the_html_plugin_reads_the_two_things_markup_needs` checks what it asks for.
        let (plugins, _) = Plugins::load(None);
        for plugin in plugins.all().iter().filter(|plugin| plugin.id != "css" && plugin.id != "html") {
            assert!(plugin.grammar.word_characters.is_empty(), "{}", plugin.id);
            assert!(!plugin.grammar.hex_colors, "{}", plugin.id);
            assert!(plugin.grammar.types.is_empty(), "{}", plugin.id);
        }
    }

    #[test]
    fn the_older_plugins_ask_for_none_of_what_the_symbols_added() {
        // The same rule for the two keys `task-1675` added: a language that does not name them is a
        // language nothing about it changed for. CSS and Mermaid are deliberately among them —
        // `--brand-hue: 280` defines a custom property by position rather than by keyword, and a
        // rule that read `:` as a definer would call every property a definition.
        let (plugins, _) = Plugins::load(None);
        for id in ["css", "mermaid"] {
            let plugin = plugins.get(id).expect(id);
            assert!(plugin.grammar.definers.is_empty(), "{id} names no definers");
            assert!(!plugin.grammar.brace_definitions, "{id} asks for no brace rule");
            assert!(!plugin.grammar.defines_symbols(), "so the entries are absent for {id}");
        }
    }

    #[test]
    fn the_four_code_plugins_say_how_their_imports_are_written() {
        // `task-1680`. The two families and what each one needs, read through the manifest reader
        // the window uses, so a key that never reached the grammar would fail here.
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        for id in ["typescript", "javascript"] {
            let grammar = &plugins.get(id).expect(id).grammar;
            assert_eq!(grammar.imports, Some(ImportStyle::Quoted), "{id}");
            assert!(grammar.import_keywords.contains(&"import".to_owned()), "{id}");
            assert!(grammar.import_omit_extension, "{id} writes ./layout, not ./layout.ts");
            assert_eq!(grammar.import_index, vec!["index".to_owned()], "{id}");
            assert_eq!(grammar.export_keyword.as_deref(), Some("export"), "{id}");
            assert!(grammar.completes_imports(), "{id}");
        }
        let css = &plugins.get("css").expect("css").grammar;
        assert_eq!(css.imports, Some(ImportStyle::Quoted));
        assert_eq!(css.import_keywords, vec!["@import".to_owned()]);
        assert!(!css.import_omit_extension, "a stylesheet names the file it imports");
        assert_eq!(css.export_keyword, None, "CSS declares nothing, so it hides nothing");

        let rust = &plugins.get("rust").expect("rust").grammar;
        assert_eq!(rust.imports, Some(ImportStyle::Path));
        assert_eq!(rust.path_separator.as_deref(), Some("::"));
        assert_eq!(rust.source_roots, vec!["src".to_owned()]);
        assert_eq!(rust.export_keyword.as_deref(), Some("pub"));
        assert_eq!(rust.path_root("crate"), Some(PathRoot::Package));
        assert_eq!(rust.path_root("self"), Some(PathRoot::Module));
        assert_eq!(rust.path_root("super"), Some(PathRoot::Parent));
        assert_eq!(rust.path_root("quill_core"), None, "a package is not a reserved word");
        assert_eq!(rust.import_index, vec!["mod".to_owned(), "lib".to_owned(), "main".to_owned()]);
    }

    #[test]
    fn the_older_plugins_ask_for_none_of_what_the_imports_added() {
        // The same rule once more, and Mermaid is what keeps it honest: a diagram imports nothing,
        // so nothing about it changed.
        let (plugins, _) = Plugins::load(None);
        let mermaid = &plugins.get("mermaid").expect("mermaid").grammar;
        assert_eq!(mermaid.imports, None);
        assert!(mermaid.import_keywords.is_empty());
        assert!(mermaid.import_extensions.is_empty());
        assert_eq!(mermaid.export_keyword, None);
        assert_eq!(mermaid.path_separator, None);
        assert!(mermaid.path_roots.is_empty());
        assert!(!mermaid.completes_imports(), "so no import is ever read out of a diagram");
    }

    /// The same rule a fifth time, for the two keys `task-1694` added. Every plugin that shipped
    /// before HTML names neither, so it is read by exactly the code that read it before — which is
    /// what keeps a `.ts` file, a stylesheet and a diagram all unchanged by a key they never asked
    /// for.
    #[test]
    fn the_older_plugins_ask_for_none_of_what_the_markup_added() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        for plugin in plugins.all().iter().filter(|plugin| plugin.id != "html") {
            assert!(!plugin.grammar.markup, "{} is not markup", plugin.id);
            assert!(plugin.grammar.raw_text.is_empty(), "{} names no raw text elements", plugin.id);
        }
    }

    /// `task-1694`. The two keys are opt-in, and this is what proves they reach the grammar at all
    /// rather than being read and dropped, the way the CSS test does for its three.
    #[test]
    fn the_html_plugin_reads_the_two_things_markup_needs() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let html = plugins.get("html").expect("the html plugin");
        assert!(html.claims(Path::new("page.html")));
        assert!(html.claims(Path::new("page.HTM")));
        assert!(!html.claims(Path::new("page.xml")), "XML is a different language, deliberately");
        assert!(html.grammar.markup, "the flag is read");
        assert!(
            html.grammar.raw_text.iter().any(|(el, lang)| el == "script" && lang.as_deref() == Some("javascript")),
            "a script block holds javascript"
        );
        assert!(
            html.grammar.raw_text.iter().any(|(el, lang)| el == "style" && lang.as_deref() == Some("css")),
            "a style block holds css"
        );
        assert!(
            html.grammar.raw_text.iter().any(|(el, lang)| el == "title" && lang.is_none()),
            "a title is escapable raw text, so it decodes its references"
        );
        assert_eq!(html.grammar.word_characters, vec!['-'], "a hyphen is a letter");
        assert_eq!(html.grammar.block_comment.as_ref(), Some(&("<!--".to_owned(), "-->".to_owned())));
        assert!(html.grammar.keywords.contains(&"p".to_owned()), "an element name is a keyword");
        assert!(html.grammar.keywords.contains(&"DOCTYPE".to_owned()), "the declaration colours");
        assert!(html.grammar.builtins.contains(&"class".to_owned()), "an attribute name is a builtin");
        assert!(html.grammar.types.is_empty(), "the third list is empty, and the type colour is the tag-name rule");

        // What that adds up to, read through the tokeniser the window uses.
        use quill_core::syntax::{highlight, Token};
        let text = "<div class=\"card\">Tom &amp; Jerry 5 < 3</div>";
        let found: Vec<(&str, Token)> = highlight(text, &html.grammar)
            .into_iter()
            .map(|(range, token)| (&text[range], token))
            .collect();
        assert!(found.contains(&("div", Token::Keyword)), "the element name: {found:?}");
        assert!(found.contains(&("class", Token::Builtin)), "the attribute name: {found:?}");
        assert!(found.contains(&("\"card\"", Token::String)), "the value: {found:?}");
        assert!(found.contains(&("&amp;", Token::Number)), "the reference in prose: {found:?}");
        assert!(
            !found.iter().any(|(word, _)| *word == "Tom" || *word == "Jerry" || *word == "5" || *word == "3"),
            "a word of the prose is not coloured: {found:?}"
        );
    }

    /// The body of a `<style>` block is coloured by the plugin that claims its language, asked at
    /// the moment of use. That is the seam `colour_the_embedded` reads, and it is what makes the
    /// withdrawal happen in the same frame rather than at the next restart.
    #[test]
    fn switching_the_css_plugin_off_withdraws_the_colouring_inside_a_style_block() {
        let (mut plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let html = plugins.get("html").expect("the html plugin");
        assert!(
            html.grammar.raw_text.iter().any(|(el, lang)| el == "style" && lang.as_deref() == Some("css")),
            "the style block names css, which is what the window asks for"
        );
        assert!(plugins.for_language("css").is_some(), "css is on, so the block is coloured");
        plugins.set_enabled(None, "css", false);
        assert!(plugins.for_language("css").is_none(), "and off, so it is not");
    }

    /// The same rule a fourth time, for the key `task-1687` added. Mermaid and CSS name no debugger,
    /// so every debug control is **absent** for their files — which is Quill's rule for a control
    /// that can never apply, and is what keeps the key opt-in.
    #[test]
    fn the_older_plugins_ask_for_none_of_what_debugging_added() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        for id in ["mermaid", "css"] {
            let plugin = plugins.get(id).expect(id);
            assert_eq!(plugin.debug_adapter, None, "{id} names no debugger");
        }
        assert_eq!(plugins.debugger_for(Path::new("a.css")), None);
        assert_eq!(plugins.debugger_for(Path::new("a.mmd")), None);
        assert_eq!(plugins.debugger_for(Path::new("a.txt")), None, "and nothing claims a .txt");
    }

    /// The three that do name one, and the two shapes they name.
    #[test]
    fn the_languages_that_can_be_debugged_name_the_debugger_that_can_do_it() {
        let (plugins, _) = Plugins::load(None);
        assert_eq!(plugins.debugger_for(Path::new("src/main.rs")), Some("lldb"));
        assert_eq!(plugins.debugger_for(Path::new("server.js")), Some("node"));
        assert_eq!(plugins.debugger_for(Path::new("server.ts")), Some("node"));
        assert!(plugins.any_debugger());
    }

    /// Asked at the moment of use, exactly as `renders` is: switching the plugin off withdraws
    /// debugging from its files in the same frame rather than at the next restart.
    #[test]
    fn switching_a_plugin_off_withdraws_debugging_from_its_files() {
        let (mut plugins, _) = Plugins::load(None);
        assert_eq!(plugins.debugger_for(Path::new("a.rs")), Some("lldb"));
        plugins.set_enabled(None, "rust", false);
        assert_eq!(plugins.debugger_for(Path::new("a.rs")), None);
    }

    /// The refusal reads the way `run.project`'s does, because it is the same decision made again.
    #[test]
    fn a_debugger_this_version_cannot_drive_is_refused_rather_than_half_loaded() {
        let head = "plugin.id = a\nlanguage.extensions = .a\n";
        let refused = parse(&Values::parse(&format!("{head}debug.adapter = gdb")), false)
            .expect_err("gdb is not one Quill drives");
        assert!(refused.contains("gdb"), "{refused}");
        assert!(refused.contains("lldb, node"), "it says what this version does drive: {refused}");
        let accepted = parse(&Values::parse(&format!("{head}debug.adapter = lldb")), false)
            .expect("one it does drive");
        assert_eq!(accepted.debug_adapter.as_deref(), Some("lldb"));
    }

    #[test]
    fn a_manifest_asking_for_an_import_shape_or_a_root_quill_does_not_have_is_refused() {
        // The rule `plugin.kind`, `language.renders` and `language.definers` already keep: a
        // manifest naming something this version does not have should say so plainly rather than
        // load as a language whose imports quietly never complete.
        let head = "plugin.id = a\nlanguage.extensions = .a\n";
        let refused = |text: &str| parse(&Values::parse(text), false).expect_err(text);
        assert!(refused(&format!("{head}language.imports = sideways")).contains("quoted or path"));
        assert!(refused(&format!("{head}language.path_roots = crate")).contains("word=meaning"));
        let unknown = format!("{head}language.path_roots = crate=universe");
        assert!(refused(&unknown).contains("package, module, parent"), "{}", refused(&unknown));
        // And the shapes that are right are read.
        let good = format!("{head}language.imports = path\nlanguage.path_roots = crate=package");
        let plugin = parse(&Values::parse(&good), false).expect("a path family language");
        assert_eq!(plugin.grammar.path_root("crate"), Some(PathRoot::Package));
    }

    #[test]
    fn the_three_code_plugins_say_which_keyword_defines_what() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let rust = plugins.get("rust").expect("rust");
        assert_eq!(rust.grammar.definer("fn"), Some(SymbolKind::Function));
        assert_eq!(rust.grammar.definer("struct"), Some(SymbolKind::Type));
        assert_eq!(rust.grammar.definer("let"), Some(SymbolKind::Variable));
        assert_eq!(rust.grammar.definer("mod"), Some(SymbolKind::Module));
        assert_eq!(rust.grammar.definer("const"), Some(SymbolKind::Constant));
        assert_eq!(rust.grammar.definer("impl"), None, "an impl block declares no name");
        assert!(!rust.grammar.brace_definitions, "Rust never hides a definition behind a brace");

        // JavaScript and TypeScript do, which is the whole reason the second key exists.
        for id in ["javascript", "typescript"] {
            let plugin = plugins.get(id).expect(id);
            assert_eq!(plugin.grammar.definer("function"), Some(SymbolKind::Function), "{id}");
            assert_eq!(plugin.grammar.definer("class"), Some(SymbolKind::Type), "{id}");
            assert_eq!(plugin.grammar.definer("const"), Some(SymbolKind::Variable), "{id}");
            assert!(plugin.grammar.brace_definitions, "{id} has methods with no keyword");
            assert!(plugin.grammar.defines_symbols());
        }
        // TypeScript adds the four words it has of its own.
        let typescript = plugins.get("typescript").expect("typescript");
        assert_eq!(typescript.grammar.definer("interface"), Some(SymbolKind::Type));
        assert_eq!(typescript.grammar.definer("enum"), Some(SymbolKind::Type));
        assert_eq!(typescript.grammar.definer("type"), Some(SymbolKind::Type));
        assert_eq!(typescript.grammar.definer("namespace"), Some(SymbolKind::Module));
        assert_eq!(plugins.get("javascript").expect("js").grammar.definer("interface"), None);
    }

    #[test]
    fn what_the_definers_add_up_to_read_through_the_reader_the_window_uses() {
        // The keys are data, so what proves they reached the grammar is what a file becomes.
        let (plugins, _) = Plugins::load(None);
        let rust = plugins.get("rust").expect("rust");
        let source = "pub fn draw(area: Rect) {}\npub struct Layout;\nconst LIMIT: usize = 4;\n";
        let found: Vec<(&str, SymbolKind)> =
            quill_core::symbols::file_definitions(source, &rust.grammar)
                .into_iter()
                .map(|definition| (&source[definition.name_range], definition.kind))
                .collect();
        assert!(found.contains(&("draw", SymbolKind::Function)), "{found:?}");
        assert!(found.contains(&("Layout", SymbolKind::Type)), "{found:?}");
        assert!(found.contains(&("LIMIT", SymbolKind::Constant)), "{found:?}");

        let typescript = plugins.get("typescript").expect("typescript");
        let source = "class Panel {\n  render(area: Rect) {\n    return area;\n  }\n}\n";
        let found: Vec<&str> = quill_core::symbols::file_definitions(source, &typescript.grammar)
            .into_iter()
            .map(|definition| &source[definition.name_range])
            .collect();
        assert_eq!(found, vec!["Panel", "render"], "the method has no keyword in front of it");
    }

    #[test]
    fn a_definers_entry_that_is_not_a_pair_or_names_an_unknown_kind_is_refused() {
        // The rule `plugin.kind` and `language.renders` already keep: a manifest asking for
        // something this version does not know says so rather than half loading.
        let text = "plugin.id = a\nlanguage.extensions = .a\nlanguage.definers = fn";
        let problem = parse(&Values::parse(text), false).expect_err("`fn` is not a pair");
        assert!(problem.contains("keyword=kind"), "{problem}");
        let text = "plugin.id = a\nlanguage.extensions = .a\nlanguage.definers = fn=gadget";
        let problem = parse(&Values::parse(text), false).expect_err("there is no gadget kind");
        assert!(problem.contains("gadget") && problem.contains("function"), "{problem}");
    }

    #[test]
    fn the_code_plugins_say_how_a_file_and_a_project_of_theirs_is_run() {
        // `task-1683` §8, and the answer to "should running node mean a Node plugin": no — node is
        // how JavaScript runs, and the JavaScript manifest says so itself.
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let javascript = plugins.get("javascript").expect("javascript");
        assert_eq!(javascript.run_file.as_deref(), Some("node {file}"));
        assert_eq!(javascript.run_project.as_deref(), Some("npm"));
        let typescript = plugins.get("typescript").expect("typescript");
        assert_eq!(typescript.run_file.as_deref(), Some("npx tsx {file}"));
        assert_eq!(typescript.run_project.as_deref(), Some("npm"));
        // Rust names a detector and no file runner: running one file of a Cargo project is not a
        // thing cargo does, so the entry is absent for a `.rs` file rather than offered and wrong.
        let rust = plugins.get("rust").expect("rust");
        assert_eq!(rust.run_file, None);
        assert_eq!(rust.run_project.as_deref(), Some("cargo"));

        // What that adds up to, asked the way the window asks it.
        assert_eq!(plugins.run_file(Path::new("server.js")), Some("node {file}"));
        assert_eq!(plugins.run_file(Path::new("main.rs")), None);
        assert_eq!(plugins.run_file(Path::new("notes.md")), None, "no plugin claims Markdown");
        // Named once, because JavaScript and TypeScript both being installed is not two projects.
        // In the order the plugins are held, which is by name, so it is the same on every run.
        assert_eq!(plugins.project_runners(), vec!["npm", "cargo"]);
    }

    #[test]
    fn switching_a_plugin_off_withdraws_its_running_in_the_same_frame() {
        // The rule `Plugins::renders` already keeps, which is what makes this a plugin rather than
        // a feature with a plugin painted on it.
        let (mut plugins, _) = Plugins::load(None);
        assert!(plugins.run_file(Path::new("server.js")).is_some());
        plugins.set_enabled(None, "javascript", false);
        assert!(plugins.run_file(Path::new("server.js")).is_none());
        // TypeScript still asks for `npm`, so the suggestions do not withdraw until it goes too.
        assert!(plugins.project_runners().contains(&"npm"));
        plugins.set_enabled(None, "typescript", false);
        assert_eq!(plugins.project_runners(), vec!["cargo"]);
    }

    #[test]
    fn the_older_plugins_ask_for_none_of_what_running_added() {
        // The rule every key since `task-1671` has followed: a language that names neither is a
        // language nothing about it changed for. CSS and Mermaid are what keep it honest — a
        // stylesheet is not run and neither is a diagram.
        let (plugins, _) = Plugins::load(None);
        for id in ["css", "mermaid"] {
            let plugin = plugins.get(id).expect(id);
            assert_eq!(plugin.run_file, None, "{id} runs no file");
            assert_eq!(plugin.run_project, None, "{id} detects no project");
        }
        assert_eq!(plugins.run_file(Path::new("site.css")), None);
        assert_eq!(plugins.run_file(Path::new("flow.mmd")), None);
    }

    #[test]
    fn a_manifest_naming_a_detector_quill_does_not_have_is_refused_with_a_reason() {
        // The rule `plugin.kind`, `language.renders`, `language.definers` and `language.imports`
        // already keep.
        let head = "plugin.id = a\nlanguage.extensions = .a\n";
        let problem = parse(&Values::parse(&format!("{head}run.project = gradle")), false)
            .expect_err("gradle is not a detector this version has");
        assert!(problem.contains("gradle"), "{problem}");
        assert!(problem.contains("cargo") && problem.contains("npm"), "and it says what there is: {problem}");
        // And a run.file with no placeholder in it, which would run the same file whatever was open.
        let problem = parse(&Values::parse(&format!("{head}run.file = node server.js")), false)
            .expect_err("a template with no placeholder");
        assert!(problem.contains("{file}"), "{problem}");
        // The shapes that are right are read.
        let good = format!("{head}run.file = ruby {{file}}\nrun.project = cargo");
        let plugin = parse(&Values::parse(&good), false).expect("a language that runs");
        assert_eq!(plugin.run_file.as_deref(), Some("ruby {file}"));
        assert_eq!(plugin.run_project.as_deref(), Some("cargo"));
    }

    #[test]
    fn a_plugin_that_is_switched_off_claims_nothing() {
        let (mut plugins, _) = Plugins::load(None);
        assert!(plugins.for_path(Path::new("a.rs")).is_some());
        plugins.set_enabled(None, "rust", false);
        assert!(plugins.for_path(Path::new("a.rs")).is_none());
        assert_eq!(plugins.enabled_count(), bundled::ALL.len() - 1);
    }

    #[test]
    fn the_mermaid_plugin_claims_diagram_files_and_names_a_renderer() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "{problems:?}");
        let mermaid = plugins.get("mermaid").expect("the mermaid plugin");
        assert_eq!(mermaid.kind, Kind::Language, "the seam is not widened: it is a language");
        assert!(mermaid.claims(Path::new("flow.mmd")));
        assert!(mermaid.claims(Path::new("flow.MERMAID")));
        assert!(!mermaid.claims(Path::new("notes.md")), "Markdown is not this plugin's business");
        assert_eq!(mermaid.renders.as_deref(), Some("mermaid"));
        assert!(plugins.renders("mermaid"), "the window asks this before it draws anything");
    }

    #[test]
    fn switching_the_mermaid_plugin_off_withdraws_the_renderer() {
        // This is what makes it a plugin rather than a feature with a plugin painted on it: the
        // window asks `renders` before it draws a diagram anywhere, so this is the whole of it.
        let (mut plugins, _) = Plugins::load(None);
        assert!(plugins.renders("mermaid"));
        plugins.set_enabled(None, "mermaid", false);
        assert!(!plugins.renders("mermaid"));
        assert!(plugins.for_path(Path::new("a.mmd")).is_none());
    }

    #[test]
    fn no_other_plugin_claims_to_render_anything() {
        let (plugins, _) = Plugins::load(None);
        for plugin in plugins.all().iter().filter(|plugin| plugin.id != "mermaid") {
            assert_eq!(plugin.renders, None, "{} should name no renderer", plugin.id);
        }
        assert!(!plugins.renders("something-else"), "a name nothing declares is not rendered");
    }

    #[test]
    fn a_manifest_naming_a_renderer_quill_does_not_have_is_refused_with_a_reason() {
        // The same rule `plugin.kind` keeps, and for the same reason: a manifest asking for a
        // picture this version cannot draw must say so rather than load as a language whose files
        // quietly never draw.
        let text = "plugin.id = a\nlanguage.extensions = .a\nlanguage.renders = holograms";
        let problem = parse(&Values::parse(text), false).expect_err("holograms are not a renderer");
        assert!(problem.contains("holograms"), "{problem}");
        assert!(problem.contains("mermaid"), "and it says what there is: {problem}");
    }

    #[test]
    fn installing_writes_the_folder_and_reads_it_back_from_disk() {
        let folder = std::env::temp_dir().join("quill-plugins-install");
        std::fs::remove_dir_all(&folder).ok();
        let store = Store::at(&folder);
        let (mut plugins, _) = Plugins::load(None);
        plugins.install(&store, "rust").expect("install");
        let written = Plugins::folder(&store, "rust");
        assert!(written.join(MANIFEST).is_file(), "the manifest is written where a person can read it");
        assert!(written.join(ICON).is_file());
        // What is loaded now came off disk, which is what proves the loader works on real files.
        let (loaded, problems) = Plugins::load(Some(&store));
        assert!(problems.is_empty(), "{problems:?}");
        let rust = loaded.get("rust").expect("rust");
        assert!(!rust.bundled, "the one on disk shadows the bundled one");
        assert_eq!(
            loaded.all().len(),
            bundled::ALL.len(),
            "shadowing replaces rather than adding a second one"
        );
    }

    #[test]
    fn a_plugin_that_is_switched_off_is_remembered() {
        let folder = std::env::temp_dir().join("quill-plugins-disabled");
        std::fs::remove_dir_all(&folder).ok();
        let store = Store::at(&folder);
        let (mut plugins, _) = Plugins::load(Some(&store));
        plugins.set_enabled(Some(&store), "javascript", false);
        let (again, _) = Plugins::load(Some(&store));
        assert!(!again.get("javascript").expect("javascript").enabled);
        assert!(again.get("rust").expect("rust").enabled);
    }

    #[test]
    fn a_folder_with_a_broken_manifest_is_skipped_and_reported() {
        let folder = std::env::temp_dir().join("quill-plugins-broken");
        std::fs::remove_dir_all(&folder).ok();
        let store = Store::at(&folder);
        let broken = store.folder().join(FOLDER).join("broken");
        std::fs::create_dir_all(&broken).expect("make the folder");
        std::fs::write(broken.join(MANIFEST), "this is not a manifest").expect("write it");
        let (plugins, problems) = Plugins::load(Some(&store));
        assert_eq!(problems.len(), 1, "the reason is reported rather than thrown away");
        assert_eq!(
            plugins.all().len(),
            bundled::ALL.len(),
            "Quill still has every one of its bundled plugins"
        );
    }
}
