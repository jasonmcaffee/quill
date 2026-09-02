//! The data sources, written down in the plugin's own folder.
//!
//! The same `services::store::Values` format the settings file and every plugin manifest use, so a
//! person can read and correct it by hand — which is the property `plugin.conf` has and the reason
//! this is not a private binary file.
//!
//! **No password is ever written here.** What is recorded is the *place* a password is: the name of
//! an environment variable, or of an entry in the machine's own keychain. That is
//! `services::agent_tasks::keychain`'s rule and Agent-Chat's, and it is what makes this file safe to
//! copy between machines, to put in a backup, and to paste into a bug report. A password somebody
//! types into the connect dialog is held in the process and is gone when the window closes.

use std::path::Path;

use quill_db::source::{Engine, Secret, Source, SslMode};

use crate::services::store::Values;

/// How many rows a result keeps before it says there are more.
///
/// IntelliJ's own default is 500 and its own settings page calls it the page size. Two hundred is a
/// screenful several times over on a 340 point pane and a second round trip is cheap; the number is a
/// setting either way.
pub const DEFAULT_PAGE_SIZE: usize = 200;

/// What a `read_only` data source is by default.
///
/// **On**, which is the opposite of IntelliJ and is deliberate: a data source added in a hurry points
/// at something real, and the cost of having to clear a tick box before the first `UPDATE` is much
/// smaller than the cost of the first `UPDATE` being one nobody meant.
pub const DEFAULT_READ_ONLY: bool = true;

/// Everything this plugin remembers.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub sources: Vec<Source>,
    /// Which data source the tree and a new console are pointed at.
    pub chosen: String,
    pub page_size: usize,
    /// Whether a statement that is not a read has to be confirmed before it is sent from a console.
    ///
    /// On by default. A console is where somebody types `delete from member` meaning to type a
    /// `where` clause after it, and one dialog is cheaper than the row that is not there any more.
    pub confirm_writes: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            chosen: String::new(),
            page_size: DEFAULT_PAGE_SIZE,
            confirm_writes: true,
        }
    }
}

impl Configuration {
    pub const FILE: &'static str = "sources.conf";

    /// Read it, and say what was refused.
    ///
    /// A row naming an engine or an ssl mode this version has not got is **left out with a sentence**
    /// rather than loaded as something else, which is the rule `plugin.kind`, `language.renders`,
    /// `run.project`, `debug.adapter`, `ui.chrome` and Agent-Chat's own wire shapes all keep.
    pub fn read(folder: &Path) -> (Self, Vec<String>) {
        let text = std::fs::read_to_string(folder.join(Self::FILE)).unwrap_or_default();
        Self::of(&Values::parse(&text))
    }

    pub fn of(values: &Values) -> (Self, Vec<String>) {
        let mut out = Configuration::default();
        let mut refused = Vec::new();
        if let Some(size) = values.number("page_size") {
            out.page_size = (size as usize).clamp(1, 100_000);
        }
        if let Some(confirm) = values.flag("confirm_writes") {
            out.confirm_writes = confirm;
        }
        out.chosen = values.text("chosen").unwrap_or_default().to_owned();
        let count = values.number("sources").unwrap_or_default() as usize;
        for index in 0..count.min(200) {
            match one(values, index) {
                Ok(source) => out.sources.push(source),
                Err(why) => refused.push(why),
            }
        }
        // A chosen name that is not a source any more points at nothing, which would leave the tree
        // with a heading and no rows. The first source is the honest fallback.
        if !out.sources.iter().any(|source| source.name == out.chosen) {
            out.chosen = out.sources.first().map(|source| source.name.clone()).unwrap_or_default();
        }
        (out, refused)
    }

    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let mut values = Values::new();
        values.set("sources", self.sources.len().to_string());
        values.set("chosen", &self.chosen);
        values.set("page_size", self.page_size.to_string());
        values.set("confirm_writes", self.confirm_writes.to_string());
        for (index, source) in self.sources.iter().enumerate() {
            let at = |key: &str| format!("source.{index}.{key}");
            values.set(&at("name"), &source.name);
            values.set(&at("engine"), source.engine.name());
            values.set(&at("read_only"), source.read_only.to_string());
            match source.engine {
                Engine::Sqlite => values.set(&at("file"), &source.database),
                Engine::Postgres => {
                    values.set(&at("host"), &source.host);
                    values.set(&at("port"), source.port.to_string());
                    values.set(&at("database"), &source.database);
                    values.set(&at("user"), &source.user);
                    values.set(&at("sslmode"), source.ssl.name());
                }
            }
            // **Where the password is, never what it is** — and a password somebody typed for this
            // window is written down as nothing at all, which is what `until this window closes`
            // means.
            match &source.secret {
                Secret::Environment(name) => values.set(&at("password.env"), name),
                Secret::Keychain(name) => values.set(&at("password.keychain"), name),
                Secret::None | Secret::Typed(_) => {}
            }
        }
        std::fs::create_dir_all(folder).map_err(|why| format!("{} could not be made: {why}", folder.display()))?;
        std::fs::write(
            folder.join(Self::FILE),
            values.to_text_headed(
                "The data sources the Database plugin knows about.\n\
                 Written by Quill; safe to edit by hand.\n\n\
                 No password is here and none ever will be: `password.env` names an environment\n\
                 variable and `password.keychain` names an entry in this machine's own keychain,\n\
                 and the value is read at the moment a connection is opened and never held.",
            ),
        )
        .map_err(|why| format!("{} could not be written: {why}", Self::FILE, ))
        .map_err(|why| format!("{why}"))
    }

    pub fn source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|source| source.name == name)
    }

    pub fn source_mut(&mut self, name: &str) -> Option<&mut Source> {
        self.sources.iter_mut().find(|source| source.name == name)
    }

    /// A name nothing else is using, so two data sources can never share one.
    ///
    /// The name is the key everywhere — the settings file, the tree, the command line — so a
    /// duplicate would make `connect ai` mean two things.
    pub fn a_free_name(&self, wanted: &str) -> String {
        let wanted = match wanted.trim().is_empty() {
            true => "data source",
            false => wanted.trim(),
        };
        if !self.sources.iter().any(|source| source.name == wanted) {
            return wanted.to_owned();
        }
        (2..)
            .map(|number| format!("{wanted} {number}"))
            .find(|name| !self.sources.iter().any(|source| source.name == *name))
            .unwrap_or_else(|| wanted.to_owned())
    }
}

/// One `source.<n>.*` block.
fn one(values: &Values, index: usize) -> Result<Source, String> {
    let at = |key: &str| format!("source.{index}.{key}");
    let name = values.text(&at("name")).unwrap_or_default().to_owned();
    if name.is_empty() {
        return Err(format!("source.{index} has no name, so nothing could refer to it."));
    }
    let engine_name = values.text(&at("engine")).unwrap_or("postgres");
    let engine = Engine::from_name(engine_name).ok_or_else(|| {
        format!(
            "`{name}` names the engine `{engine_name}`, and this version of Quill speaks {}.",
            Engine::ALL.join(", ")
        )
    })?;
    let mut source = Source { name, engine, ..Source::default() };
    source.read_only = values.flag(&at("read_only")).unwrap_or(DEFAULT_READ_ONLY);
    match engine {
        Engine::Sqlite => {
            source.database = values.text(&at("file")).unwrap_or_default().to_owned();
            source.host = String::new();
            source.port = 0;
            if source.database.is_empty() {
                return Err(format!("`{}` names no file, so there is nothing to open.", source.name));
            }
        }
        Engine::Postgres => {
            source.host = values.text(&at("host")).unwrap_or("localhost").to_owned();
            source.port = values.number(&at("port")).unwrap_or(5432.0) as u16;
            source.database = values.text(&at("database")).unwrap_or_default().to_owned();
            source.user = values.text(&at("user")).unwrap_or_default().to_owned();
            let mode = values.text(&at("sslmode")).unwrap_or("prefer");
            source.ssl = SslMode::from_name(mode).ok_or_else(|| {
                format!(
                    "`{}` names sslmode `{mode}`, and Quill has {}.",
                    source.name,
                    SslMode::ALL.join(", ")
                )
            })?;
        }
    }
    // A file that names a password rather than a place is refused loudly, because it is the one
    // mistake that would put a secret on disk and nobody would notice it had.
    if values.text(&at("password")).is_some() {
        return Err(format!(
            "`{}` has a `password` in the file. Quill does not store one: use `password.env` to \
             name an environment variable instead. The line has been ignored.",
            source.name
        ));
    }
    source.secret = match (values.text(&at("password.env")), values.text(&at("password.keychain"))) {
        (Some(name), _) if !name.is_empty() => Secret::Environment(name.to_owned()),
        (_, Some(name)) if !name.is_empty() => Secret::Keychain(name.to_owned()),
        _ => Secret::None,
    };
    Ok(source)
}

/// The password for a data source, read at the moment a connection is opened.
///
/// Never held anywhere: the caller passes it straight to `Worker::open` and it is dropped when that
/// returns. `Secret::Typed` is the one that lives longer, and it lives in the process only — see
/// `source::Secret`.
pub fn password_for(source: &Source) -> Option<String> {
    match &source.secret {
        Secret::None => None,
        Secret::Environment(name) => std::env::var(name).ok().filter(|value| !value.is_empty()),
        Secret::Keychain(name) => crate::services::agent_tasks::keychain::read(name),
        Secret::Typed(value) => Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_folder(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("quill-database-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_configuration_round_trips_and_carries_no_secret() {
        let folder = a_folder("round-trip");
        let mut configuration = Configuration::default();
        let mut postgres = Source::parse("ai", "postgres://postgres@localhost:5432/ai").expect("read");
        postgres.secret = Secret::Environment("QUILL_DB_AI".to_owned());
        postgres.read_only = false;
        configuration.sources.push(postgres);
        // A typed password is deliberately **not** written: `until this window closes` is what the
        // dialog promises, and this is where that promise is kept.
        let mut typed = Source::parse("other", "postgres://me@db.example.com/x").expect("read");
        typed.secret = Secret::Typed("hunter2".to_owned());
        configuration.sources.push(typed);
        configuration.sources.push(Source::sqlite("tasks", r"C:\tmp\tasks.db"));
        configuration.chosen = "ai".to_owned();
        configuration.write(&folder).expect("written");

        let text = std::fs::read_to_string(folder.join(Configuration::FILE)).expect("the file");
        assert!(text.contains("QUILL_DB_AI"), "the name of the variable is written down");
        assert!(!text.contains("hunter2"), "and a typed password is not: {text}");
        let (read, refused) = Configuration::read(&folder);
        assert!(refused.is_empty(), "{refused:?}");
        assert_eq!(read.sources.len(), 3);
        assert_eq!(read.sources[0].secret, Secret::Environment("QUILL_DB_AI".to_owned()));
        assert_eq!(read.sources[1].secret, Secret::None, "a typed password does not survive the window");
        assert_eq!(read.sources[2].engine, Engine::Sqlite);
        assert_eq!(read.chosen, "ai");
    }

    #[test]
    fn a_row_naming_an_engine_this_version_has_not_got_is_left_out_with_a_sentence() {
        let (configuration, refused) = Configuration::of(&Values::parse(
            "sources = 2\n\
             source.0.name = mine\n\
             source.0.engine = mysql\n\
             source.1.name = ok\n\
             source.1.engine = sqlite\n\
             source.1.file = C:/tmp/x.db\n",
        ));
        assert_eq!(configuration.sources.len(), 1);
        assert_eq!(configuration.sources[0].name, "ok");
        assert!(refused[0].contains("postgres, sqlite"), "{refused:?}");
    }

    #[test]
    fn a_password_written_into_the_file_by_hand_is_refused_loudly() {
        // The one mistake that would put a secret on disk without anybody noticing.
        let (configuration, refused) = Configuration::of(&Values::parse(
            "sources = 1\n\
             source.0.name = mine\n\
             source.0.engine = postgres\n\
             source.0.user = me\n\
             source.0.password = hunter2\n",
        ));
        assert!(configuration.sources.is_empty());
        assert!(refused[0].contains("does not store one"), "{refused:?}");
        assert!(!refused[0].contains("hunter2"), "and the refusal does not quote it");
    }

    #[test]
    fn a_new_data_source_is_read_only_until_somebody_says_otherwise() {
        // The opposite of IntelliJ, on purpose: a data source added in a hurry points at something
        // real, and clearing a tick box is cheaper than the first `UPDATE` nobody meant.
        let (configuration, _) = Configuration::of(&Values::parse(
            "sources = 1\nsource.0.name = x\nsource.0.engine = sqlite\nsource.0.file = C:/x.db\n",
        ));
        assert!(configuration.sources[0].read_only);
    }

    #[test]
    fn two_data_sources_can_never_share_a_name() {
        let mut configuration = Configuration::default();
        configuration.sources.push(Source::sqlite("tasks", "a.db"));
        assert_eq!(configuration.a_free_name("tasks"), "tasks 2");
        configuration.sources.push(Source::sqlite("tasks 2", "b.db"));
        assert_eq!(configuration.a_free_name("tasks"), "tasks 3");
        assert_eq!(configuration.a_free_name("other"), "other");
        assert_eq!(configuration.a_free_name("  "), "data source");
    }

    #[test]
    fn a_chosen_name_that_is_not_a_source_any_more_falls_back_to_the_first() {
        let (configuration, _) = Configuration::of(&Values::parse(
            "sources = 1\nchosen = gone\nsource.0.name = here\nsource.0.engine = sqlite\nsource.0.file = a.db\n",
        ));
        assert_eq!(configuration.chosen, "here");
    }
}
