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
    /// The built-in renderer this language's files are drawn with, if it has one.
    pub renders: Option<String>,
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

    /// True when some plugin that is switched on asks for the built-in renderer called `name`.
    ///
    /// The window asks this before it draws a diagram anywhere — a `.mmd` file's preview, and every
    /// mermaid block in a Markdown document — so switching the plugin off withdraws the feature in
    /// the same frame rather than at the next restart.
    pub fn renders(&self, name: &str) -> bool {
        self.installed
            .iter()
            .any(|plugin| plugin.enabled && plugin.renders.as_deref() == Some(name))
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
        other => return Err(format!("plugin.kind is `{other}`, and this version of Quill only runs `language` plugins")),
    };
    let extensions: Vec<String> = list(values, "language.extensions")
        .into_iter()
        .map(|extension| extension.trim_start_matches('.').to_lowercase())
        .collect();
    if extensions.is_empty() {
        return Err("language.extensions is empty, so nothing would ever use this plugin".to_owned());
    }
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
        renders,
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
        (
            "mermaid",
            include_str!("../../plugins/mermaid/plugin.conf"),
            Some(include_bytes!("../../plugins/mermaid/icon.png")),
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
    fn the_bundled_plugins_all_parse_and_claim_what_they_should() {
        let (plugins, problems) = Plugins::load(None);
        assert!(problems.is_empty(), "a bundled plugin should always parse: {problems:?}");
        let ids: Vec<&str> = plugins.all().iter().map(|plugin| plugin.id.as_str()).collect();
        assert!(ids.contains(&"javascript") && ids.contains(&"typescript") && ids.contains(&"rust"));
        assert!(ids.contains(&"mermaid") && ids.contains(&"css"));
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
        let (plugins, _) = Plugins::load(None);
        for plugin in plugins.all().iter().filter(|plugin| plugin.id != "css") {
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
