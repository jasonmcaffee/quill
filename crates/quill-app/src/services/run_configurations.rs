//! A run configuration is a **named command**, and this is everything that is true about one
//! without a window: the four fields, the splitter that turns a command line into a program and its
//! arguments, the file beside the project that holds them, the temporaries, and the two built-in
//! project detectors.
//!
//! `tasks/task-1683-run-configurations-tdd.md` is the design. Everything here is pure — no egui, no
//! `Session`, nothing started — so all of it is tested with a temporary folder and no window, which
//! is the seam `services::file_search` and `services::file_move` already keep.
//!
//! ## One kind, not a template per language
//!
//! IntelliJ has several dozen configuration *types*, each a form of its own; §2.2 of the TDD walks
//! two of them and finds that both compose into one command line wearing six boxes. So there is no
//! `type` field here:
//!
//! ```text
//! name         Dev server
//! command      node server.js --port 3000
//! directory    backend
//! env          PORT=3000; DEBUG=app:*
//! ```
//!
//! **No shell runs the command line.** [`split_command`] does the splitting and the parts are handed
//! to the process as arguments, so nothing expands, nothing globs, and a command containing `&&` is
//! one program with a strange argument rather than two programs. Somebody who wants a shell writes
//! `pwsh -Command ...` and has said so in the one place it can be seen.
//!
//! ## Where they live
//!
//! `.quill/run-configurations.conf`, read by [`crate::services::store::Values`] like every other
//! file Quill writes, numbered the way `files.panes` already numbers a list:
//!
//! ```text
//! run.1.name = Dev server
//! run.1.command = node server.js --port 3000
//! run.1.directory = backend
//! run.1.env = PORT=3000; DEBUG=app:*
//! run.2.name = cargo run
//! run.2.command = cargo run
//! ```
//!
//! One file for the whole project rather than one per configuration, which is what `highlights.txt`
//! already chose. It is in `.quill` because a run configuration belongs to the **project** — the
//! command that starts this server is a fact about this folder. What is per-person goes where
//! per-person things go: `workspace.conf` holds `run.selected` and `run.visible`.
//!
//! A `run.N` block missing its name or its command is dropped **whole**, with the rule the project
//! state keeps: a project that opens with one configuration missing is better than a project that
//! will not open.

use std::path::{Path, PathBuf};

use crate::services::store::Values;

/// The file inside `.quill` that holds them.
pub const FILE: &str = "run-configurations.conf";

/// How many temporary configurations are kept before the oldest is dropped.
///
/// IntelliJ's number, and for its reason: a list of everything that has ever been run is a list
/// nobody reads. They are the ones nobody deliberately made, so five of them is a memory rather
/// than a collection.
pub const TEMPORARY_LIMIT: usize = 5;

/// What `{file}` stands for in a plugin's `run.file`.
pub const FILE_PLACEHOLDER: &str = "{file}";

/// One configuration: a name, a command line, a folder and some variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configuration {
    pub name: String,
    /// The whole command line, RustRover-style: the first word is the program and the rest are its
    /// arguments. See [`split_command`].
    pub command: String,
    /// Relative to the project root, empty meaning the root itself — the rule
    /// `.quill/open-files.txt` already follows, for the same reason: the project may move.
    pub directory: String,
    /// `NAME=value` pairs separated by semicolons, RustRover's spelling. See [`parse_env`].
    pub env: String,
}

impl Configuration {
    /// A configuration with nothing in it but a name and a command, which is what a detector and a
    /// temporary both are.
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self { name: name.into(), command: command.into(), ..Self::default() }
    }

    /// The program to start and the arguments to hand it, or nothing when the command is empty.
    pub fn program_and_arguments(&self) -> Option<(String, Vec<String>)> {
        let mut parts = split_command(&self.command).into_iter();
        let program = parts.next()?;
        Some((program, parts.collect()))
    }

    /// The variables to lay over the environment Quill started with.
    pub fn environment(&self) -> Vec<(String, String)> {
        parse_env(&self.env)
    }

    /// Which folder the program starts in, resolved against the project.
    pub fn working_directory(&self, root: &Path) -> PathBuf {
        resolve_directory(root, &self.directory)
    }

    /// True when this is enough to start something.
    pub fn is_runnable(&self) -> bool {
        !self.name.trim().is_empty() && self.program_and_arguments().is_some()
    }
}

/// Whether the program this command names can be found, without starting anything.
///
/// `task-1691` reported a configuration of `node primes.js` that `run add` accepted without comment
/// and `run start` then failed on, because a window launched from Finder has no version manager's
/// directory on its `PATH`. An agent writes `node`, `python` or `cargo`, and those are exactly the
/// programs a version manager keeps off a desktop-launched application's `PATH` — so the answer is
/// worth having at the moment the configuration is written down rather than only when it is run.
///
/// It is a **question**, not a refusal. A configuration may name a program that will exist by the
/// time it is run, and a `run add` that refused one would be a `run add` nobody could use to write
/// down what they are about to install. What the caller does with the answer is the caller's.
///
/// A program with a separator in it is a path, resolved against the configuration's own folder;
/// anything else is looked for on `PATH`, with `PATHEXT` on Windows because `node` there is really
/// `node.exe` or `node.cmd`. Nothing is spawned and nothing is executed to find out.
pub fn found_on_path(program: &str, directory: &Path) -> bool {
    if program.trim().is_empty() {
        return false;
    }
    if program.contains('/') || program.contains('\\') {
        let named = Path::new(program);
        let against = match named.is_absolute() {
            true => named.to_path_buf(),
            false => directory.join(named),
        };
        return with_extensions(&against).any(|candidate| candidate.is_file());
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .filter(|folder| !folder.as_os_str().is_empty())
        .any(|folder| with_extensions(&folder.join(program)).any(|candidate| candidate.is_file()))
}

/// The spellings of one program name that the platform will actually run.
///
/// One on every platform but Windows, where a bare name is completed from `PATHEXT` — and where the
/// name as written is tried first, so a command naming `node.exe` outright is not looked for as
/// `node.exe.COM`.
fn with_extensions(candidate: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let mut spellings = vec![candidate.to_path_buf()];
    #[cfg(windows)]
    {
        let already = candidate
            .extension()
            .map(|extension| !extension.is_empty())
            .unwrap_or(false);
        if !already {
            let listed = std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
            for extension in listed.split(';').filter(|part| !part.trim().is_empty()) {
                let mut name = candidate.as_os_str().to_owned();
                name.push(extension.trim());
                spellings.push(PathBuf::from(name));
            }
        }
    }
    spellings.into_iter()
}

/// Where a configuration came from, which is what decides how it is drawn and whether it is written
/// down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Made deliberately and kept in `.quill/run-configurations.conf` until it is removed.
    Permanent,
    /// Made by running a file or a suggestion. In memory only, capped at [`TEMPORARY_LIMIT`], and
    /// **never written to disk**: IntelliJ writes its temporaries into `workspace.xml` and Quill
    /// deliberately does not, because a file the project shares should hold what somebody chose to
    /// keep. `Save` in the dialog promotes one.
    Temporary,
    /// Offered by a built-in project detector — `cargo run`, `npm run build`. Not held at all: it is
    /// worked out from the folder every time it is asked for, and running one makes a temporary.
    Suggested,
}

/// The configurations a project has, permanent and temporary.
///
/// The suggestions are deliberately not in here: they are derived from the folder and the plugins
/// that are switched on, so holding them would be holding a copy of something that can change under
/// it. [`detect`] answers that question at the moment of use, which is the rule
/// `Plugins::renders` already keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunConfigurations {
    permanent: Vec<Configuration>,
    temporary: Vec<Configuration>,
}

impl RunConfigurations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn permanent(&self) -> &[Configuration] {
        &self.permanent
    }

    pub fn temporary(&self) -> &[Configuration] {
        &self.temporary
    }

    pub fn is_empty(&self) -> bool {
        self.permanent.is_empty() && self.temporary.is_empty()
    }

    pub fn len(&self) -> usize {
        self.permanent.len() + self.temporary.len()
    }

    /// Every configuration this project holds, permanents first, each with where it came from.
    ///
    /// The order the widget's flyout and the dialog's list both draw, so the two cannot come to
    /// disagree about what "the second one" is.
    pub fn listed(&self) -> Vec<(Origin, &Configuration)> {
        self.permanent
            .iter()
            .map(|configuration| (Origin::Permanent, configuration))
            .chain(self.temporary.iter().map(|configuration| (Origin::Temporary, configuration)))
            .collect()
    }

    /// The configuration of this name, and where it came from. Permanents are looked at first, so a
    /// temporary that has been saved does not shadow the thing it became.
    pub fn find(&self, name: &str) -> Option<(Origin, &Configuration)> {
        self.listed().into_iter().find(|(_, configuration)| configuration.name == name)
    }

    /// The same, to be edited. `None` for a name nothing holds.
    pub fn find_mut(&mut self, name: &str) -> Option<&mut Configuration> {
        if let Some(at) = self.permanent.iter().position(|held| held.name == name) {
            return self.permanent.get_mut(at);
        }
        let at = self.temporary.iter().position(|held| held.name == name)?;
        self.temporary.get_mut(at)
    }

    /// Keep a configuration for good. A name that is already permanent is replaced rather than
    /// added a second time, because two rows with one name is a list nobody can choose from.
    pub fn add_permanent(&mut self, configuration: Configuration) {
        self.temporary.retain(|held| held.name != configuration.name);
        match self.permanent.iter().position(|held| held.name == configuration.name) {
            Some(at) => self.permanent[at] = configuration,
            None => self.permanent.push(configuration),
        }
    }

    /// Remember something that was run without being filled in first: `Run Current File`, or a
    /// detector's suggestion.
    ///
    /// A name that is already permanent is left alone — running a permanent configuration must not
    /// quietly make a second, temporary copy of it — and the oldest is dropped past
    /// [`TEMPORARY_LIMIT`].
    pub fn add_temporary(&mut self, configuration: Configuration) {
        if self.permanent.iter().any(|held| held.name == configuration.name) {
            return;
        }
        self.temporary.retain(|held| held.name != configuration.name);
        self.temporary.push(configuration);
        while self.temporary.len() > TEMPORARY_LIMIT {
            self.temporary.remove(0);
        }
    }

    /// Turn a temporary into a permanent, which is what `Save` in the dialog does.
    ///
    /// Returns false for a name that is not a temporary, so a caller can say so rather than looking
    /// as though it worked.
    pub fn promote(&mut self, name: &str) -> bool {
        let Some(at) = self.temporary.iter().position(|held| held.name == name) else {
            return false;
        };
        let configuration = self.temporary.remove(at);
        self.add_permanent(configuration);
        true
    }

    /// Take a configuration away, wherever it was held.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.len();
        self.permanent.retain(|held| held.name != name);
        self.temporary.retain(|held| held.name != name);
        self.len() != before
    }

    /// A name nothing holds, built from `wanted` with a number after it if it has to be.
    ///
    /// What `Add` in the dialog and `run add` on the command line both use, so a second
    /// `Unnamed` is `Unnamed 2` rather than a refusal or a duplicate.
    pub fn unused_name(&self, wanted: &str) -> String {
        if self.find(wanted).is_none() {
            return wanted.to_owned();
        }
        for number in 2.. {
            let candidate = format!("{wanted} {number}");
            if self.find(&candidate).is_none() {
                return candidate;
            }
        }
        unreachable!("there is always a number nothing is called")
    }
}

// ------------------------------------------------------------------------------- the command line

/// Split a command line into a program and its arguments, the way a shell splits a double-quoted
/// word — and **nothing else**.
///
/// Whitespace separates, a `"` groups, and `\"` inside a quoted part is a literal quote. A lone
/// backslash is a backslash: on Windows every second path has one in it, and a rule that ate them
/// would make `C:\Program Files\node\node.exe` unwriteable, which is the one thing this has to get
/// right. A single quote is an ordinary character for the same reason — it is a letter in a great
/// many file names and means nothing to `cmd`.
///
/// Nothing is expanded and nothing is refused. `&&` comes out as an argument, because there is no
/// shell here to read it as anything else, and that is stated in the dialog rather than discovered.
pub fn split_command(command: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                // An empty pair of quotes is an empty argument somebody asked for, so the part is
                // marked as started even though nothing was added to it.
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted && characters.peek() == Some(&'"') => {
                characters.next();
                current.push('"');
                started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            character => {
                current.push(character);
                started = true;
            }
        }
    }
    if started {
        parts.push(current);
    }
    parts
}

/// The other direction: a program and its arguments back into one command line.
///
/// [`split_command`]'s inverse, and it has to be exactly that — a debug adapter's `runInTerminal`
/// hands over a program and its arguments **already split**, and the run tile takes a command line,
/// so a part with a space in it that came back unquoted would be run as two programs. Every part
/// goes through [`quote_part`], which is the one place that question is answered.
pub fn join_command(program: &str, args: &[String]) -> String {
    let mut line = quote_part(program);
    for argument in args {
        line.push(' ');
        line.push_str(&quote_part(argument));
    }
    line
}

/// Write a part back as it would have to be typed, which is what a detector does with a path.
///
/// Only what has to be quoted is quoted, so `cargo run` stays `cargo run` rather than becoming
/// something a person would not have written.
pub fn quote_part(part: &str) -> String {
    if !part.is_empty() && !part.chars().any(|character| character.is_whitespace() || character == '"')
    {
        return part.to_owned();
    }
    format!("\"{}\"", part.replace('"', "\\\""))
}

/// Read `NAME=value; OTHER=value` into pairs.
///
/// Semicolons separate, the first `=` divides, and the spaces round both are trimmed. An entry with
/// no `=` in it is dropped rather than becoming a variable with no value: a person who typed
/// `DEBUG` meant something, and guessing which of the two things it was is worse than leaving it
/// out. An empty value is kept, because `QUIET=` is a thing programs read.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| entry.split_once('='))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// The other way round, for a caller that has pairs and needs the field.
pub fn format_env(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Which folder a configuration runs in.
///
/// Relative to the project root, empty meaning the root itself. A path that is somehow absolute is
/// used as it is, which is what a person who typed one meant.
pub fn resolve_directory(root: &Path, directory: &str) -> PathBuf {
    let directory = directory.trim();
    if directory.is_empty() {
        return root.to_path_buf();
    }
    let path = Path::new(directory);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

// ------------------------------------------------------------------------------------ the file

/// Read what this project holds. A file that cannot be read is a file that is not there.
pub fn load(root: &Path) -> RunConfigurations {
    let path = crate::services::project_state::folder(root).join(FILE);
    let Ok(text) = std::fs::read_to_string(path) else {
        return RunConfigurations::new();
    };
    read(&Values::parse(&text))
}

/// The same, from values that have already been parsed, which is what a test hands in.
pub fn read(values: &Values) -> RunConfigurations {
    let mut configurations = RunConfigurations::new();
    // Numbered from one, and the numbers need not be contiguous — a person may have deleted a
    // block by hand. A generous ceiling rather than a walk of every key, because `Values` is a map
    // of names and this is read once when a project opens.
    for number in 1..=999 {
        let named = |field: &str| values.text(&format!("run.{number}.{field}"));
        let (Some(name), Some(command)) = (named("name"), named("command")) else {
            continue;
        };
        let (name, command) = (name.trim(), command.trim());
        // A block missing its name or its command is dropped whole. Half a configuration is a row
        // in the widget that cannot be run, which is worse than a row that is not there.
        if name.is_empty() || command.is_empty() {
            continue;
        }
        configurations.add_permanent(Configuration {
            name: name.to_owned(),
            command: command.to_owned(),
            directory: named("directory").unwrap_or_default().trim().to_owned(),
            env: named("env").unwrap_or_default().trim().to_owned(),
        });
    }
    configurations
}

/// Write the permanents down. The temporaries are deliberately not written — see [`Origin`].
///
/// A failure is reported on the error output and otherwise ignored, which is the rule the settings
/// file and the project state already keep: a read-only folder is not a reason to stop editing.
pub fn save(root: &Path, configurations: &RunConfigurations) {
    let folder = crate::services::project_state::folder(root);
    if let Err(problem) = std::fs::create_dir_all(&folder) {
        eprintln!("Quill could not make {}: {problem}", folder.display());
        return;
    }
    let path = folder.join(FILE);
    if let Err(problem) = std::fs::write(&path, write(configurations)) {
        eprintln!("Quill could not write {}: {problem}", path.display());
    }
}

/// What the file holds, as text. Split out so a test can read it without a folder.
pub fn write(configurations: &RunConfigurations) -> String {
    let mut values = Values::new();
    for (at, configuration) in configurations.permanent().iter().enumerate() {
        let number = at + 1;
        values.set(format!("run.{number}.name").as_str(), configuration.name.clone());
        values.set(format!("run.{number}.command").as_str(), configuration.command.clone());
        // Written only when they hold something, so the commonest configuration is two lines rather
        // than four with two of them empty.
        if !configuration.directory.trim().is_empty() {
            values.set(format!("run.{number}.directory").as_str(), configuration.directory.clone());
        }
        if !configuration.env.trim().is_empty() {
            values.set(format!("run.{number}.env").as_str(), configuration.env.clone());
        }
    }
    values.to_text_headed(
        "# The run configurations for this project. Written by Quill, and safe to edit by hand.",
    )
}

// -------------------------------------------------------------------------------- the detectors

/// What a project of this shape offers, worked out from the folder every time it is asked.
///
/// `runners` are the detector names the plugins that are switched on have asked for — see
/// `services::plugins::PROJECT_RUNNERS`. A detector named twice runs once, because two plugins
/// claiming `npm` is JavaScript and TypeScript both being installed rather than two projects.
///
/// The parsing is Quill's own code, shipped in the binary, which is what keeps a third-party
/// manifest from being able to smuggle logic: the most a manifest can do is name a detector this
/// version already has.
pub fn detect(root: &Path, runners: &[&str]) -> Vec<Configuration> {
    let mut found: Vec<Configuration> = Vec::new();
    let mut done: Vec<&str> = Vec::new();
    for runner in runners {
        if done.contains(runner) {
            continue;
        }
        done.push(runner);
        match *runner {
            "cargo" => found.extend(detect_cargo(root)),
            "npm" => found.extend(detect_npm(root)),
            // A name this version does not have never gets here: `plugins::parse` refuses a
            // manifest that asks for one, exactly as it refuses an unknown renderer.
            _ => {}
        }
    }
    found
}

/// `cargo run`, when the project root holds a `Cargo.toml`.
///
/// One suggestion rather than one per binary target: reading the manifest for `[[bin]]` sections
/// would need a TOML parser, and `cargo run` is what a person types in a project with one binary,
/// which is nearly all of them. Somebody with several writes `cargo run --bin thing` in a
/// configuration of their own, which is the field being one command line rather than a form.
fn detect_cargo(root: &Path) -> Vec<Configuration> {
    if !root.join("Cargo.toml").is_file() {
        return Vec::new();
    }
    vec![Configuration::new("cargo run", "cargo run")]
}

/// `npm run <script>` for each script in the project root's `package.json`.
///
/// The scripts are what the person who wrote the project decided is worth running, which is a
/// better list than anything Quill could guess at. A `package.json` that will not parse, or that
/// has no scripts, offers nothing rather than complaining: a suggestion is a convenience, and a
/// project with a broken manifest has a problem this is not the place to report.
///
/// They come out **in name order** rather than in the order the file writes them, because
/// `serde_json` holds an object's keys in a `BTreeMap`. That is worth knowing rather than working
/// around: a list that is the same every time is what makes a picture of the flyout a test, and
/// alphabetical is as good an order as the one somebody's `package.json` happens to be in.
fn detect_npm(root: &Path) -> Vec<Configuration> {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(scripts) = parsed.get("scripts").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    scripts
        .keys()
        .map(|script| Configuration::new(format!("npm run {script}"), format!("npm run {script}")))
        .collect()
}

/// The temporary configuration `Run Current File` makes.
///
/// `template` is the plugin's `run.file` — `node {file}`, `npx tsx {file}` — and the placeholder
/// becomes the path, relative to the project where it is inside it so a project that moves still
/// runs. It is quoted only if it has to be, which is [`quote_part`]'s rule.
///
/// It is named after the file rather than after the command, because the widget shows the name and
/// `server.js` says more there than `node server.js` does.
pub fn for_file(template: &str, root: &Path, file: &Path) -> Configuration {
    let relative = crate::services::project_state::relative(root, file);
    let written = quote_part(&relative.to_string_lossy());
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| relative.to_string_lossy().to_string());
    Configuration::new(name, template.replace(FILE_PLACEHOLDER, &written))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).expect("make the project");
        root
    }

    #[test]
    fn a_command_line_is_split_into_a_program_and_its_arguments() {
        assert_eq!(split_command("node server.js --port 3000"), vec![
            "node", "server.js", "--port", "3000"
        ]);
        assert_eq!(split_command("cargo run"), vec!["cargo", "run"]);
        assert_eq!(split_command("   spaced    out   "), vec!["spaced", "out"]);
        assert!(split_command("").is_empty());
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn a_path_with_a_space_in_it_is_written_in_quotes() {
        assert_eq!(
            split_command("\"C:\\Program Files\\node\\node.exe\" server.js"),
            vec!["C:\\Program Files\\node\\node.exe", "server.js"]
        );
        // And the backslashes survive, which is the one thing this has to get right on Windows: a
        // rule that ate them would make half the paths on the machine unwriteable.
        assert_eq!(split_command("C:\\tools\\thing.exe"), vec!["C:\\tools\\thing.exe"]);
    }

    #[test]
    fn a_quote_inside_a_quoted_part_is_escaped_with_a_backslash() {
        assert_eq!(split_command("say \"a \\\"quoted\\\" word\""), vec!["say", "a \"quoted\" word"]);
        // An empty pair of quotes is an argument somebody asked for.
        assert_eq!(split_command("thing \"\" after"), vec!["thing", "", "after"]);
    }

    #[test]
    fn a_single_quote_is_an_ordinary_character() {
        // It is a letter in a great many file names and means nothing to `cmd`, so treating it as a
        // quote would break more than it fixed.
        assert_eq!(split_command("open jason's notes.txt"), vec!["open", "jason's", "notes.txt"]);
    }

    #[test]
    fn nothing_in_a_command_line_is_expanded_or_refused() {
        // No shell runs it, so `&&` is one program with a strange argument rather than two
        // programs, and a wildcard is a wildcard rather than a list of files.
        assert_eq!(split_command("npm run build && npm test"), vec![
            "npm", "run", "build", "&&", "npm", "test"
        ]);
        assert_eq!(split_command("rm *.log"), vec!["rm", "*.log"]);
    }

    #[test]
    fn a_configuration_says_what_to_start_and_with_what() {
        let configuration = Configuration::new("Dev server", "node server.js --port 3000");
        let (program, arguments) = configuration.program_and_arguments().expect("a program");
        assert_eq!(program, "node");
        assert_eq!(arguments, vec!["server.js", "--port", "3000"]);
        assert!(configuration.is_runnable());
        // A command with nothing in it is not something that can be started, and says so rather
        // than starting a program called nothing.
        let empty = Configuration::new("Nothing", "   ");
        assert_eq!(empty.program_and_arguments(), None);
        assert!(!empty.is_runnable());
    }

    #[test]
    fn environment_pairs_are_read_and_written_back() {
        let pairs = parse_env("PORT=3000; DEBUG=app:*");
        assert_eq!(pairs, vec![
            ("PORT".to_owned(), "3000".to_owned()),
            ("DEBUG".to_owned(), "app:*".to_owned()),
        ]);
        assert_eq!(format_env(&pairs), "PORT=3000; DEBUG=app:*");
        // A value with an `=` in it keeps it: only the first one divides.
        assert_eq!(parse_env("URL=https://x/?a=b"), vec![
            ("URL".to_owned(), "https://x/?a=b".to_owned())
        ]);
        // An empty value is kept, because `QUIET=` is a thing programs read; an entry with no `=`
        // at all is dropped rather than guessed at.
        assert_eq!(parse_env("QUIET=; DEBUG"), vec![("QUIET".to_owned(), String::new())]);
        assert!(parse_env("   ").is_empty());
    }

    #[test]
    fn a_relative_directory_is_resolved_against_the_project() {
        let root = Path::new("/project");
        assert_eq!(resolve_directory(root, "backend"), root.join("backend"));
        assert_eq!(resolve_directory(root, "  "), root.to_path_buf(), "empty means the root itself");
        let absolute = if cfg!(target_os = "windows") { "C:\\elsewhere" } else { "/elsewhere" };
        assert_eq!(resolve_directory(root, absolute), PathBuf::from(absolute));
    }

    #[test]
    fn the_configurations_survive_being_written_and_read_back() {
        let root = project("quill-run-round-trip");
        let mut configurations = RunConfigurations::new();
        configurations.add_permanent(Configuration {
            name: "Dev server".to_owned(),
            command: "node server.js --port 3000".to_owned(),
            directory: "backend".to_owned(),
            env: "PORT=3000; DEBUG=app:*".to_owned(),
        });
        configurations.add_permanent(Configuration::new("cargo run", "cargo run"));
        save(&root, &configurations);
        assert_eq!(load(&root), configurations);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_temporary_is_never_written_down() {
        // IntelliJ writes its temporaries into `workspace.xml`; Quill deliberately does not,
        // because a file the project shares should hold what somebody chose to keep.
        let root = project("quill-run-temporaries-not-written");
        let mut configurations = RunConfigurations::new();
        configurations.add_permanent(Configuration::new("Dev server", "node server.js"));
        configurations.add_temporary(Configuration::new("one.js", "node one.js"));
        save(&root, &configurations);
        let read_back = load(&root);
        assert_eq!(read_back.permanent().len(), 1);
        assert!(read_back.temporary().is_empty(), "and the temporary is gone with the window");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_block_missing_its_command_is_dropped_whole() {
        let values = Values::parse(
            "run.1.name = Dev server
run.1.command = node server.js
run.2.name = Broken
run.2.directory = backend
run.3.command = cargo run
run.4.name = Fine
run.4.command = cargo test
",
        );
        let configurations = read(&values);
        let names: Vec<&str> =
            configurations.permanent().iter().map(|held| held.name.as_str()).collect();
        assert_eq!(names, vec!["Dev server", "Fine"], "the halves are dropped, the wholes are kept");
    }

    #[test]
    fn a_file_that_cannot_be_read_is_a_file_that_is_not_there() {
        let root = project("quill-run-missing");
        assert!(load(&root).is_empty());
        // And one full of something that is not a configuration file opens the project with none
        // rather than refusing to open it.
        let folder = crate::services::project_state::folder(&root);
        std::fs::create_dir_all(&folder).expect("make the state folder");
        std::fs::write(folder.join(FILE), "this is not a configuration file").expect("write it");
        assert!(load(&root).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn temporaries_are_capped_at_five_with_the_oldest_dropped() {
        let mut configurations = RunConfigurations::new();
        for number in 1..=7 {
            configurations
                .add_temporary(Configuration::new(format!("file{number}.js"), "node x.js"));
        }
        let names: Vec<&str> =
            configurations.temporary().iter().map(|held| held.name.as_str()).collect();
        assert_eq!(names, vec!["file3.js", "file4.js", "file5.js", "file6.js", "file7.js"]);
        assert_eq!(configurations.temporary().len(), TEMPORARY_LIMIT);
    }

    #[test]
    fn running_the_same_file_again_moves_it_up_rather_than_listing_it_twice() {
        let mut configurations = RunConfigurations::new();
        for name in ["one.js", "two.js", "three.js"] {
            configurations.add_temporary(Configuration::new(name, format!("node {name}")));
        }
        configurations.add_temporary(Configuration::new("one.js", "node one.js"));
        let names: Vec<&str> =
            configurations.temporary().iter().map(|held| held.name.as_str()).collect();
        assert_eq!(names, vec!["two.js", "three.js", "one.js"]);
    }

    #[test]
    fn running_a_permanent_configuration_does_not_make_a_temporary_copy_of_it() {
        let mut configurations = RunConfigurations::new();
        configurations.add_permanent(Configuration::new("Dev server", "node server.js"));
        configurations.add_temporary(Configuration::new("Dev server", "node server.js"));
        assert!(configurations.temporary().is_empty());
        assert_eq!(configurations.permanent().len(), 1);
    }

    #[test]
    fn saving_a_temporary_makes_it_permanent_and_takes_it_out_of_the_temporaries() {
        let mut configurations = RunConfigurations::new();
        configurations.add_temporary(Configuration::new("server.js", "node server.js"));
        assert!(configurations.promote("server.js"));
        assert_eq!(configurations.permanent().len(), 1);
        assert!(configurations.temporary().is_empty());
        assert_eq!(configurations.find("server.js").map(|(origin, _)| origin), Some(Origin::Permanent));
        assert!(!configurations.promote("server.js"), "it is not a temporary any more");
        assert!(!configurations.promote("nothing"));
    }

    #[test]
    fn a_configuration_is_found_by_name_and_taken_away_by_name() {
        let mut configurations = RunConfigurations::new();
        configurations.add_permanent(Configuration::new("Dev server", "node server.js"));
        configurations.add_temporary(Configuration::new("one.js", "node one.js"));
        assert_eq!(configurations.find("Dev server").map(|(origin, _)| origin), Some(Origin::Permanent));
        assert_eq!(configurations.find("one.js").map(|(origin, _)| origin), Some(Origin::Temporary));
        assert!(configurations.find("nothing").is_none());
        assert!(configurations.remove("one.js"));
        assert!(!configurations.remove("one.js"));
        assert_eq!(configurations.len(), 1);
    }

    #[test]
    fn the_list_reads_permanents_first_and_temporaries_after_them() {
        let mut configurations = RunConfigurations::new();
        configurations.add_temporary(Configuration::new("one.js", "node one.js"));
        configurations.add_permanent(Configuration::new("Dev server", "node server.js"));
        let listed: Vec<(Origin, &str)> = configurations
            .listed()
            .into_iter()
            .map(|(origin, configuration)| (origin, configuration.name.as_str()))
            .collect();
        assert_eq!(listed, vec![
            (Origin::Permanent, "Dev server"),
            (Origin::Temporary, "one.js"),
        ]);
    }

    #[test]
    fn a_name_that_is_already_taken_gets_a_number() {
        let mut configurations = RunConfigurations::new();
        assert_eq!(configurations.unused_name("Unnamed"), "Unnamed");
        configurations.add_permanent(Configuration::new("Unnamed", "cargo run"));
        assert_eq!(configurations.unused_name("Unnamed"), "Unnamed 2");
        configurations.add_permanent(Configuration::new("Unnamed 2", "cargo run"));
        assert_eq!(configurations.unused_name("Unnamed"), "Unnamed 3");
    }

    #[test]
    fn the_cargo_detector_offers_a_run_when_the_project_has_a_manifest() {
        let root = project("quill-run-detect-cargo");
        assert!(detect(&root, &["cargo"]).is_empty(), "nothing to detect in an empty folder");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"thing\"\n").expect("write it");
        let found = detect(&root, &["cargo"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "cargo run");
        assert_eq!(found[0].command, "cargo run");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_npm_detector_offers_one_run_for_each_script_in_the_package() {
        let root = project("quill-run-detect-npm");
        std::fs::write(
            root.join("package.json"),
            "{\n  \"name\": \"thing\",\n  \"scripts\": { \"build\": \"tsc\", \"start\": \"node server.js\" }\n}\n",
        )
        .expect("write it");
        let found = detect(&root, &["npm"]);
        let names: Vec<&str> = found.iter().map(|held| held.name.as_str()).collect();
        // In name order, whatever order the file writes them in, so the list is the same on every
        // run — which is what makes a picture of the widget's flyout a test.
        assert_eq!(names, vec!["npm run build", "npm run start"]);
        assert_eq!(found[0].command, "npm run build");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_package_with_no_scripts_or_one_that_will_not_parse_offers_nothing() {
        let root = project("quill-run-detect-npm-empty");
        std::fs::write(root.join("package.json"), "{ \"name\": \"thing\" }").expect("write it");
        assert!(detect(&root, &["npm"]).is_empty());
        std::fs::write(root.join("package.json"), "{ not json at all").expect("write it");
        assert!(detect(&root, &["npm"]).is_empty(), "a broken manifest is not a complaint here");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_detector_named_twice_runs_once() {
        // JavaScript and TypeScript both say `run.project = npm`, and both being installed is not
        // two projects.
        let root = project("quill-run-detect-twice");
        std::fs::write(root.join("package.json"), "{ \"scripts\": { \"build\": \"tsc\" } }")
            .expect("write it");
        assert_eq!(detect(&root, &["npm", "npm"]).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_detector_name_this_version_does_not_have_offers_nothing() {
        // It can never get this far — `plugins::parse` refuses a manifest naming one — and this is
        // what keeps that true rather than a panic if it ever did.
        let root = project("quill-run-detect-unknown");
        assert!(detect(&root, &["gradle"]).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn run_current_file_puts_the_path_where_the_placeholder_was() {
        let root = Path::new("/project");
        let configuration = for_file("node {file}", root, &root.join("src").join("server.js"));
        assert_eq!(configuration.name, "server.js", "named after the file, which is what is shown");
        let expected = format!("node {}", Path::new("src").join("server.js").display());
        assert_eq!(configuration.command, expected, "and relative, so a project that moves still runs");
        assert!(configuration.directory.is_empty(), "from the project root");
    }

    #[test]
    fn a_file_with_a_space_in_its_name_is_quoted_when_it_is_put_in_the_command() {
        let root = Path::new("/project");
        let configuration = for_file("npx tsx {file}", root, &root.join("my notes.ts"));
        assert_eq!(configuration.command, "npx tsx \"my notes.ts\"");
        // And it splits back into the two parts it was built from.
        assert_eq!(split_command(&configuration.command), vec!["npx", "tsx", "my notes.ts"]);
    }

    #[test]
    fn a_program_is_looked_for_on_the_path_and_a_path_is_looked_for_where_it_points() {
        // `task-1691`. Nothing is spawned to find out, so this runs anywhere.
        let folder = std::env::temp_dir().join("quill-run-found-on-path");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        let program = folder.join(if cfg!(windows) { "tool.exe" } else { "tool" });
        std::fs::write(&program, b"not really a program").expect("write it");

        assert!(!found_on_path("definitely-not-a-real-program", &folder));
        assert!(!found_on_path("", &folder));
        // An absolute path is looked for where it points, whatever the `PATH` says.
        assert!(found_on_path(&program.to_string_lossy(), Path::new("/somewhere/else")));
        // A relative one is resolved against the configuration's own folder, and the separator is
        // what makes it a path rather than a name.
        let relative = format!("./{}", program.file_name().expect("a name").to_string_lossy());
        assert!(found_on_path(&relative, &folder));
        assert!(!found_on_path(&relative, Path::new("/somewhere/else")));
        // And the real thing: whatever started this test is on the `PATH`.
        assert!(found_on_path("cargo", &folder), "cargo is what started this test");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_name_on_windows_is_completed_from_pathext() {
        // `node` on Windows is really `node.exe` or `node.cmd`, so a walk that looked for the name
        // as written would say every program on the machine was missing.
        let folder = std::env::temp_dir().join("quill-run-pathext");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(folder.join("thing.cmd"), b"@echo off").expect("write it");
        let bare = folder.join("thing");
        assert!(with_extensions(&bare).any(|candidate| candidate.is_file()));
        // A name already carrying its extension is not looked for as `thing.cmd.COM`.
        let spelled: Vec<_> = with_extensions(&folder.join("thing.cmd")).collect();
        assert_eq!(spelled, vec![folder.join("thing.cmd")]);
        std::fs::remove_dir_all(&folder).ok();
    }
}
