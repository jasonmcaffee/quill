//! The window's side of the UI plugins: which providers are open, and which of their panes are showing.
//!
//! `services::plugins` says what a manifest contributed and `services::plugin_ui` says what a provider
//! is. This is the third piece: the window has to hold one provider per plugin that draws, know which
//! slot each contributed pane is in, and build the provider the first time somebody presses its button
//! rather than at startup.
//!
//! ## Nothing is built until it is asked for
//!
//! [`PluginUi::opened`] is the only thing that builds a provider, and it is called from the rail button,
//! the tab and the Settings page. A plugin nobody opens costs one row in a list. That is the reference editor's own
//! arrangement, whose documented reason is the same: a tool window a person never clicks loads and runs
//! no plugin code.
//!
//! ## A pane's slot is the window's business
//!
//! `dock::Panel::Plugin(0)` is the first contributed pane. Which pane that is comes from
//! `plugins::Surfaces`, which is worked out from the manifests, so it changes when a plugin is switched
//! on or off. The settings file records a pane's side against its own `<plugin id>/<pane id>` rather
//! than against its slot, so installing a second plugin does not move the first one's pane.

use std::path::PathBuf;

use crate::app::dock::{Panel, PLUGIN_PANES};
use crate::services::plugin_ui::{self, Context, UiProvider};
use crate::services::plugins::{Plugins, Surfaces};

/// One provider, and whether it has been opened.
struct Loaded {
    /// The `plugin.id` of the plugin whose manifest named it.
    plugin: String,
    provider: Box<dyn UiProvider>,
    /// What went wrong when it was opened, if it did. Drawn in the pane rather than thrown away.
    problem: Option<String>,
}

/// Every plugin that draws, and what is showing.
#[derive(Default)]
pub struct PluginUi {
    loaded: Vec<Loaded>,
    /// The contributed panes, tabs, menus and pages, in the order the plugins are listed.
    surfaces: Surfaces,
    /// The panes that are showing, by their own `<plugin id>/<pane id>`.
    ///
    /// **By name rather than by slot**, for the reason their sides and sizes are recorded by name: which
    /// slot a pane is in comes from the manifests and moves when a plugin is switched on or off. Keyed by
    /// slot, switching off the first of two plugins would leave the second one's pane showing or hidden
    /// according to what the first one was doing.
    showing: Vec<String>,
    /// The folder each plugin keeps its own files in, worked out once when the settings folder is known.
    settings_folder: Option<PathBuf>,
    /// The project this window has open, which a provider is told about when it is opened.
    project: Option<PathBuf>,
    /// The file showing, told to a provider when it opens. See `plugin_ui::Context::showing`.
    showing_file: Option<PathBuf>,
    /// The folders this machine has had open, newest first, told to a provider the same way.
    recent_projects: Vec<PathBuf>,
    /// How a provider asks the window to draw again from another thread.
    wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for PluginUi {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("PluginUi")
            .field("loaded", &self.loaded.iter().map(|one| one.plugin.as_str()).collect::<Vec<&str>>())
            .field("panes", &self.surfaces.panes.len())
            .field("showing", &self.showing)
            .finish()
    }
}

impl PluginUi {
    /// Read what the enabled plugins contribute, and drop any provider whose plugin has gone.
    ///
    /// Called when the plugins are loaded and again whenever one is switched on or off, which is what
    /// makes a contribution appear and disappear in the same frame. A provider whose plugin was switched
    /// off is closed, so it drops what it held — an open database file, in Agent-Tasks' case.
    pub fn refresh(&mut self, plugins: &Plugins) {
        self.surfaces = plugins.surfaces();
        let still_contributing = self.surfaces.plugins();
        // A provider whose plugin has gone is closed, so it drops what it held — an open board file, in
        // Agent-Tasks' case. Closed before it is dropped rather than left to `Drop`, because closing is a
        // thing the provider does and dropping is a thing that happens to it.
        for one in &mut self.loaded {
            if !still_contributing.contains(&one.plugin) {
                one.provider.close();
            }
        }
        self.loaded.retain(|one| still_contributing.contains(&one.plugin));
        // A pane whose plugin has gone cannot still be showing, and neither can one whose condition has
        // stopped being met — a project closing is the case that matters. Held by name, so this is the one
        // place a pane stops showing and a slot moving underneath it changes nothing.
        let still_shown: Vec<String> = (0..self.pane_count())
            .filter(|slot| self.applies(*slot))
            .filter_map(|slot| self.pane_key(slot))
            .collect();
        self.showing.retain(|open| still_shown.contains(open));
    }

    pub fn surfaces(&self) -> &Surfaces {
        &self.surfaces
    }

    /// Where the plugins keep their own files, which is `<settings folder>/plugins`.
    pub fn set_settings_folder(&mut self, folder: PathBuf) {
        self.settings_folder = Some(folder);
    }

    pub fn set_project(&mut self, project: Option<PathBuf>) {
        self.project = project;
    }

    /// Which file is showing, so a provider opened later is told at once rather than at the next
    /// change. See `plugin_ui::Context::showing`.
    pub fn set_showing(&mut self, showing: Option<PathBuf>) {
        self.showing_file = showing;
    }

    /// The recent projects, for a provider that offers them as choices. See `plugin_ui::Context`.
    pub fn set_recent_projects(&mut self, projects: Vec<PathBuf>) {
        self.recent_projects = projects;
    }

    /// How a provider asks for a frame from another thread, handed on when one is opened.
    pub fn set_waker(&mut self, wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.wake = Some(wake);
    }

    /// How many panes are contributed, which is what `dock::Panel::all` is asked for.
    pub fn pane_count(&self) -> usize {
        self.surfaces.panes.len().min(PLUGIN_PANES)
    }

    /// Whether the pane in `slot` applies just now, which is `pane.applies` in its manifest.
    ///
    /// **A control that cannot apply is absent**, which is Unluminous's rule everywhere: the `F` button is not
    /// drawn for a `.rs` file and the three code navigation entries are not on the Edit menu for a
    /// stylesheet. So a pane whose condition is not met has no button in the rail and cannot be shown,
    /// rather than having a button that reports a refusal.
    ///
    /// Two conditions, checked against `plugins::PANE_CONDITIONS` when the manifest was read, so an
    /// unknown one was refused there and cannot reach here.
    pub fn applies(&self, slot: usize) -> bool {
        match self.pane(slot).map(|pane| pane.applies.as_str()) {
            Some("in_project") => self.project.is_some(),
            // `always`, and anything the reader let through, which is only `always`.
            Some(_) => true,
            None => false,
        }
    }

    /// The `<plugin id>/<pane id>` of the pane in `slot`.
    pub fn pane_key(&self, slot: usize) -> Option<String> {
        self.surfaces.panes.get(slot).map(|surface| surface.key(&surface.what.id))
    }

    /// Every contributed pane's name, in slot order, for the settings file.
    pub fn pane_keys(&self) -> Vec<String> {
        (0..self.pane_count()).filter_map(|slot| self.pane_key(slot)).collect()
    }

    /// The slot the pane named `key` is in.
    pub fn slot_of(&self, key: &str) -> Option<usize> {
        (0..self.pane_count()).find(|slot| self.pane_key(*slot).as_deref() == Some(key))
    }

    /// What the manifest said about the pane in `slot`.
    pub fn pane(&self, slot: usize) -> Option<&crate::services::plugins::PaneContribution> {
        self.surfaces.panes.get(slot).map(|surface| &surface.what)
    }

    pub fn is_visible(&self, slot: usize) -> bool {
        self.pane_key(slot).is_some_and(|key| self.showing.contains(&key))
    }

    /// Which contributed panes are showing, for `dock::regions`, in slot order.
    pub fn visible(&self) -> [bool; PLUGIN_PANES] {
        let mut visible = [false; PLUGIN_PANES];
        for (slot, showing) in visible.iter_mut().enumerate() {
            *showing = self.is_visible(slot);
        }
        visible
    }

    /// Show or hide the pane in `slot`, building its provider the first time it is shown.
    pub fn set_visible(&mut self, slot: usize, showing: bool) -> Option<String> {
        if slot >= self.pane_count() {
            return Some(format!("there is no plugin pane in slot {slot}"));
        }
        if showing {
            if !self.applies(slot) {
                return Some(format!(
                    "{} asks for a project to be open, and this window has none",
                    self.pane(slot).map(|pane| pane.label.clone()).unwrap_or_default()
                ));
            }
            let problem = self.open_slot(slot);
            if problem.is_some() {
                return problem;
            }
        }
        let Some(key) = self.pane_key(slot) else {
            return Some(format!("there is no plugin pane in slot {slot}"));
        };
        self.showing.retain(|open| *open != key);
        if showing {
            self.showing.push(key);
        }
        None
    }

    /// The provider behind the pane in `slot`, built if it has not been yet.
    fn open_slot(&mut self, slot: usize) -> Option<String> {
        let plugin = self.surfaces.panes.get(slot)?.plugin.clone();
        let provider = self.surfaces.panes.get(slot)?.provider.clone();
        self.opened(&plugin, &provider).err()
    }

    /// The provider named `provider` for the plugin `plugin`, opened.
    ///
    /// The one place a provider is built and the one place `open` is called, so "opened" cannot mean two
    /// different things in two places, and a provider that failed to open keeps its reason.
    pub fn opened(
        &mut self,
        plugin: &str,
        provider: &str,
    ) -> Result<&mut (dyn UiProvider + 'static), String> {
        if let Some(index) = self.loaded.iter().position(|one| one.plugin == plugin) {
            if let Some(problem) = self.loaded[index].problem.clone() {
                return Err(problem);
            }
            return Ok(self.loaded[index].provider.as_mut());
        }
        let mut built = plugin_ui::provider(provider)
            .ok_or_else(|| format!("this version of Unluminous has no `{provider}` provider"))?;
        let context = Context {
            project: self.project.clone(),
            showing: self.showing_file.clone(),
            recent_projects: self.recent_projects.clone(),
            folder: self.settings_folder.as_ref().map(|folder| folder.join(plugin)),
            wake: self.wake.clone(),
        };
        let problem = built.open(&context).err();
        self.loaded.push(Loaded { plugin: plugin.to_owned(), provider: built, problem: problem.clone() });
        match problem {
            Some(problem) => Err(problem),
            None => Ok(self.loaded.last_mut().expect("just pushed").provider.as_mut()),
        }
    }

    /// The provider for `plugin`, if it has been opened. Nothing is built here.
    pub fn provider(&mut self, plugin: &str) -> Option<&mut (dyn UiProvider + 'static)> {
        let one =
            self.loaded.iter_mut().find(|one| one.plugin == plugin && one.problem.is_none())?;
        Some(one.provider.as_mut())
    }

    /// The provider for `plugin` without changing it, for reading its view.
    pub fn view_of(&self, plugin: &str) -> Option<serde_json::Value> {
        self.loaded
            .iter()
            .find(|one| one.plugin == plugin && one.problem.is_none())
            .map(|one| one.provider.view())
    }

    /// Why this plugin's pane is empty, if it is.
    pub fn problem_with(&self, plugin: &str) -> Option<&str> {
        self.loaded
            .iter()
            .find(|one| one.plugin == plugin)
            .and_then(|one| one.problem.as_deref())
    }

    /// True when this plugin's provider has been built and opened.
    pub fn is_open(&self, plugin: &str) -> bool {
        self.loaded.iter().any(|one| one.plugin == plugin && one.provider.is_open())
    }

    /// The plugin a slot's pane belongs to.
    pub fn plugin_of(&self, slot: usize) -> Option<String> {
        self.surfaces.panes.get(slot).map(|surface| surface.plugin.clone())
    }

    /// Run one command against a plugin's provider, opening it first if it has not been opened.
    ///
    /// The one path a plugin command goes down, whichever of the three ways in asked for it: a menu
    /// entry, a button inside a pane, or `unluminous-cli plugin run`.
    pub fn run(
        &mut self,
        plugin: &str,
        command: &str,
        arguments: &[String],
    ) -> Result<plugin_ui::Answer, String> {
        let provider = self
            .surfaces
            .provider_of(plugin)
            .ok_or_else(|| format!("`{plugin}` is not a plugin that draws, or it is switched off"))?;
        let opened = self.opened(plugin, &provider)?;
        opened.command(command, arguments)
    }

    /// Close every provider, which is what happens when the window closes or the project changes.
    pub fn close(&mut self) {
        for one in &mut self.loaded {
            one.provider.close();
        }
        self.loaded.clear();
        self.showing.clear();
    }
}

impl Panel {
    /// The pane a contributed slot holds, as a `Panel`, when there is one.
    pub fn plugin_pane(slot: usize) -> Option<Panel> {
        (slot < PLUGIN_PANES).then_some(Panel::Plugin(slot as u8))
    }
}
