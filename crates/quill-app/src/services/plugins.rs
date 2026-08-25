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
//! ## The manifest
//!
//! `plugin.conf`, in the same `name = value` format the settings file already uses, read by the same
//! [`crate::services::store::Values`]. No new dependency, and a plugin can be read and corrected in
//! a text editor, which is fitting in a text editor.

use std::path::{Path, PathBuf};

use quill_core::syntax::{Grammar, Token};
use quill_core::Color;

use crate::services::store::{Store, Values};

/// The folder under the settings folder that installed plugins live in.
const FOLDER: &str = "plugins";
/// The file inside a plugin folder that describes it.
const MANIFEST: &str = "plugin.conf";
/// The picture a plugin puts in front of its files.
const ICON: &str = "icon.png";

/// The kind of plugin. One today, and the field exists so that a second one can be refused rather
/// than half-loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A description of a language: extensions, a grammar, an icon and a colour scheme.
    Language,
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
    /// The extensions it claims, without the dot, in lower case.
    pub extensions: Vec<String>,
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

    pub fn enabled_count(&self) -> usize {
        self.installed.iter().filter(|plugin| plugin.enabled).count()
    }

    /// The plugin that claims `path`, if one does and it is switched on.
    pub fn for_path(&self, path: &Path) -> Option<&Plugin> {
        self.installed.iter().find(|plugin| plugin.enabled && plugin.claims(path))
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
        other => return Err(format!("plugin.kind is `{other}`, and this version of Quill only runs `language` plugins")),
    };
    let extensions: Vec<String> = list(values, "language.extensions")
        .into_iter()
        .map(|extension| extension.trim_start_matches('.').to_lowercase())
        .collect();
    if extensions.is_empty() {
        return Err("language.extensions is empty, so nothing would ever use this plugin".to_owned());
    }
    let name = values.text("plugin.name").unwrap_or(&id).to_owned();
    let grammar = Grammar {
        language: name.clone(),
        keywords: list(values, "language.keywords"),
        builtins: list(values, "language.builtins"),
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
        extensions,
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

    #[test]
    fn the_three_bundled_plugins_all_parse_and_claim_what_they_should() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "a bundled plugin should always parse: {problems:?}");
        let ids: Vec<&str> = plugins.all().iter().map(|plugin| plugin.id.as_str()).collect();
        assert!(ids.contains(&"javascript") && ids.contains(&"typescript") && ids.contains(&"rust"));
        assert_eq!(plugins.for_path(Path::new("a.rs")).map(|p| p.id.as_str()), Some("rust"));
        assert_eq!(plugins.for_path(Path::new("a.ts")).map(|p| p.id.as_str()), Some("typescript"));
        assert_eq!(plugins.for_path(Path::new("a.js")).map(|p| p.id.as_str()), Some("javascript"));
        assert_eq!(plugins.for_path(Path::new("a.md")), None, "Markdown is not a plugin's business");
        // Every one of them ships an icon and a colour scheme, which is what the ticket asks for.
        for plugin in plugins.all() {
            assert!(plugin.icon.is_some(), "{} has no icon", plugin.id);
            assert!(!plugin.theme.is_empty(), "{} has no colour scheme", plugin.id);
            assert!(!plugin.description.is_empty(), "{} says nothing about itself", plugin.id);
        }
    }

    #[test]
    fn a_plugin_that_is_switched_off_claims_nothing() {
        let (mut plugins, _) = Plugins::load(None);
        assert!(plugins.for_path(Path::new("a.rs")).is_some());
        plugins.set_enabled(None, "rust", false);
        assert!(plugins.for_path(Path::new("a.rs")).is_none());
        assert_eq!(plugins.enabled_count(), 2);
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
        assert_eq!(loaded.all().len(), 3, "shadowing replaces rather than adding a second one");
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
        assert_eq!(plugins.all().len(), 3, "Quill still has its three bundled plugins");
    }
}
