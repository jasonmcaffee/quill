//! A plugin's own configuration, reachable from `settings`.
//!
//! `task-1804` §4.2 measured the gap this closes: *"`settings list` names 23 keys and none of them
//! belong to the Agent-Chat or the Database plugin. A person sets a chat row's program, URL, model
//! and key-variable, and a data source and where its password lives, through Settings pages; an
//! agent has no route to any of it. Against the rule the product is built on, these are the two
//! places it is not kept."*
//!
//! ## Why this is general rather than two commands
//!
//! Because the fault is general. Every `ui` plugin keeps its configuration in a `settings.conf`
//! inside its own folder, read through the same [`Values`] store the window's own settings file uses
//! — the Agent-Chat pane, the Agent-Tasks board and the Database plugin all do. A `chat` area and a
//! `database` area in the catalogue would have answered for two of them and left the next one in
//! exactly the state these were in.
//!
//! So `settings` reaches **any** plugin's file, under `plugins.<plugin id>.<key>`, and a plugin
//! added tomorrow is agent-configurable the day it is added — which is the same argument
//! `actions::menus` makes about a menu entry needing nothing, and `mcp::tools` about a command
//! becoming a tool.
//!
//! ## Two things it must not do
//!
//! **It must not become a route to a secret.** It cannot be one, and that is a property of the files
//! rather than of this code: `services::agent_tasks::keychain` says a secret must not go in a
//! settings file, so what a chat row holds is *the name of the environment variable* its key comes
//! from and what a data source holds is *where* its password is kept. Both are already what
//! `plugins view` and the Settings pages show. A plugin that broke that rule would be broken with or
//! without this file, and `a_settings_file_holds_the_name_of_a_secret_and_never_a_secret` is what
//! says so out loud.
//!
//! **It must not invent keys.** What is listed is what is *in the file*, so a plugin that has never
//! been configured lists nothing rather than a made-up set of defaults it might not agree with. The
//! defaults live in each plugin's own `Configuration::default`, where they belong, and a key appears
//! here once it has a value.

use std::path::{Path, PathBuf};

use crate::services::store::Values;

/// The file every plugin keeps its configuration in, inside its own folder.
pub const FILE: &str = "settings.conf";

/// The prefix a plugin's keys are reached under.
const PREFIX: &str = "plugins.";

/// One key of one plugin's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The plugin's id, as `plugins list` says it.
    pub plugin: String,
    /// The key inside that plugin's own file.
    pub key: String,
}

impl Key {
    /// The name `settings` calls it: `plugins.agent-chat.provider.0.program`.
    pub fn name(&self) -> String {
        format!("{PREFIX}{}.{}", self.plugin, self.key)
    }
}

/// Read `plugins.<plugin>.<key>` out of a settings name, against the plugins there are.
///
/// The plugin's id is matched against the list rather than taken as everything up to the first dot,
/// because a plugin id may hold dots and a key certainly does — `provider.0.program` is three. The
/// **longest** matching id wins, so `themes-bundle-1` is not read as a plugin called `themes`.
pub fn read(name: &str, plugins: &[String]) -> Option<Key> {
    let rest = name.strip_prefix(PREFIX)?;
    let mut best: Option<&String> = None;
    for plugin in plugins {
        let Some(after) = rest.strip_prefix(plugin.as_str()) else {
            continue;
        };
        if !after.starts_with('.') || after.len() < 2 {
            continue;
        }
        if best.is_none_or(|found| plugin.len() > found.len()) {
            best = Some(plugin);
        }
    }
    let plugin = best?;
    let key = rest[plugin.len() + 1..].to_owned();
    Some(Key { plugin: plugin.clone(), key })
}

/// Where a plugin's configuration file is.
pub fn path(folder: &Path, plugin: &str) -> PathBuf {
    folder.join(plugin).join(FILE)
}

/// What is in one plugin's file, in the order it is written.
///
/// Nothing when the plugin has never been configured, which is not an error: a plugin with no file
/// is one nobody has changed, and its behaviour is its own defaults.
pub fn values(folder: &Path, plugin: &str) -> Values {
    match std::fs::read_to_string(path(folder, plugin)) {
        Ok(text) => Values::parse(&text),
        Err(_) => Values::new(),
    }
}

/// Every key of every plugin that has a file, as `settings list` prints them.
///
/// In name order within each plugin, which is `Values`' own order and the right one here: a listing
/// somebody reads twice should be the same both times, whatever order the file happens to hold.
pub fn every(folder: &Path, plugins: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for plugin in plugins {
        for (key, value) in values(folder, plugin).starting_with("") {
            out.push((Key { plugin: plugin.clone(), key }.name(), value));
        }
    }
    out
}

/// Write one key into a plugin's own file, keeping everything else in it.
///
/// **Merged rather than rewritten**, which is the rule `settings::save_with` already keeps about the
/// window's own file: a value set here must not take away a value set in the Settings page a moment
/// before, and a plugin's file may hold keys this version of Unluminous has never heard of.
///
/// An empty value **removes** the key, so a setting can be put back to the plugin's own default
/// rather than pinned to an empty string. That is `Values::set_or_clear`'s rule and it is why the
/// listing above shows what is in the file rather than a set of invented defaults.
pub fn write(folder: &Path, plugin: &str, key: &str, value: &str) -> std::io::Result<()> {
    let file = path(folder, plugin);
    let mut values = values(folder, plugin);
    values.set_or_clear(key, value);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, values.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugins() -> Vec<String> {
        ["agent-chat", "database", "themes-bundle-1", "themes"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    }

    #[test]
    fn a_name_is_read_into_a_plugin_and_a_key() {
        let read = read("plugins.agent-chat.provider.0.program", &plugins()).expect("it reads");
        assert_eq!(read.plugin, "agent-chat");
        assert_eq!(read.key, "provider.0.program", "a key may hold dots, and this one holds two");
        assert_eq!(read.name(), "plugins.agent-chat.provider.0.program");
    }

    /// The longest id wins, or `themes-bundle-1` would be read as `themes` with a key beginning
    /// `bundle-1`.
    #[test]
    fn the_longest_plugin_id_wins() {
        let read = read("plugins.themes-bundle-1.chosen", &plugins()).expect("it reads");
        assert_eq!(read.plugin, "themes-bundle-1");
        assert_eq!(read.key, "chosen");
    }

    #[test]
    fn a_name_that_is_not_a_plugins_key_is_nothing() {
        assert_eq!(read("appearance.font.size", &plugins()), None);
        assert_eq!(read("plugins.chrome", &plugins()), None, "a real setting of the window's own");
        assert_eq!(read("plugins.nothing.at-all", &plugins()), None);
        assert_eq!(read("plugins.agent-chat", &plugins()), None, "a plugin with no key is not a key");
        assert_eq!(read("plugins.agent-chat.", &plugins()), None, "and neither is an empty one");
    }

    #[test]
    fn what_is_listed_is_what_is_in_the_file_and_a_plugin_with_no_file_lists_nothing() {
        let folder = folder_for("listing");
        std::fs::create_dir_all(folder.join("agent-chat")).expect("make the plugin folder");
        std::fs::write(
            folder.join("agent-chat").join(FILE),
            "tools = true\nprovider.0.program = claude\n",
        )
        .expect("write it");
        let listed = every(&folder, &plugins());
        assert_eq!(
            listed,
            vec![
                ("plugins.agent-chat.provider.0.program".to_owned(), "claude".to_owned()),
                ("plugins.agent-chat.tools".to_owned(), "true".to_owned()),
            ],
            "in name order, and nothing at all for the plugins with no file"
        );
    }

    #[test]
    fn a_written_key_is_merged_rather_than_replacing_the_file() {
        let folder = folder_for("merging");
        std::fs::create_dir_all(folder.join("database")).expect("make the plugin folder");
        std::fs::write(
            folder.join("database").join(FILE),
            "source.0.name = library\nsource.0.engine = sqlite\n",
        )
        .expect("write it");
        write(&folder, "database", "source.0.name", "books").expect("write the key");
        let after = values(&folder, "database");
        assert_eq!(after.text("source.0.name"), Some("books"));
        assert_eq!(after.text("source.0.engine"), Some("sqlite"), "the rest of the file is still there");
    }

    #[test]
    fn an_empty_value_takes_the_key_out_so_the_plugins_own_default_comes_back() {
        let folder = folder_for("clearing");
        write(&folder, "agent-chat", "system", "be brief").expect("write it");
        assert_eq!(values(&folder, "agent-chat").text("system"), Some("be brief"));
        write(&folder, "agent-chat", "system", "").expect("clear it");
        assert_eq!(values(&folder, "agent-chat").text("system"), None);
    }

    #[test]
    fn writing_into_a_plugin_that_has_no_folder_yet_makes_one() {
        let folder = folder_for("fresh");
        write(&folder, "agent-chat", "tools", "true").expect("it makes the folder");
        assert!(path(&folder, "agent-chat").is_file());
        assert_eq!(values(&folder, "agent-chat").text("tools"), Some("true"));
    }

    /// **The rule this whole file rests on**, said out loud where somebody adding a plugin will read
    /// it: a settings file holds the *name of the place* a secret is, never the secret.
    ///
    /// It is `services::agent_tasks::keychain`'s rule and both shipped `ui` plugins keep it — a chat
    /// row names an environment variable, a data source names a credential-store entry. This is a
    /// test of the two shapes rather than of a mechanism, because there is no mechanism to test: the
    /// files are what they are, and a plugin that wrote a secret into one would be broken with or
    /// without a way to read it back.
    #[test]
    fn a_settings_file_holds_the_name_of_a_secret_and_never_a_secret() {
        let folder = folder_for("secrets");
        std::fs::create_dir_all(folder.join("agent-chat")).expect("make the plugin folder");
        std::fs::write(
            folder.join("agent-chat").join(FILE),
            "provider.0.name = local\nprovider.0.key = ANTHROPIC_API_KEY\n",
        )
        .expect("write it");
        let listed = every(&folder, &plugins());
        let key = listed
            .iter()
            .find(|(name, _)| name.ends_with(".provider.0.key"))
            .expect("the row is there");
        assert_eq!(key.1, "ANTHROPIC_API_KEY", "the name of the variable, not what is in it");
    }

    fn folder_for(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("unluminous-plugin-settings-{name}"));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("make the test folder");
        folder
    }
}
