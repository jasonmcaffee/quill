//! Writing Unluminous into an agent's own configuration.
//!
//! Two clients on this machine, two formats, and one rule that applies to both: **use the client's
//! own command when it is on the path, and edit the file only when it is not.** Both files are
//! written by a program that may be running right now — `~/.claude.json` is rewritten by every
//! Claude Code session and is over a hundred kilobytes of somebody's settings — so an edit
//! underneath one is how somebody loses their configuration. `claude mcp add-json` and
//! `codex mcp add` are the supported ways in and they do the locking.
//!
//! The fallback is still written, because a machine with the editor installed and not the agent's
//! CLI is an ordinary machine, and "install it yourself then" is not an answer. It takes a copy of
//! the file first and changes as little as it can: the JSON is parsed, one key set and the whole
//! written back; the TOML has one table appended or replaced and every other line left exactly as
//! it was, because it holds comments and project trust settings that no serialiser would give back.
//!
//! Worth knowing, and measured rather than assumed: **`codex mcp add` rewrites the whole file**. It
//! reorders the keys inside a table, reflows an array onto one line and writes `120` back as
//! `120.0`. That is Codex's own doing and is exactly what happens when a person runs the command
//! themselves, so it is not a reason to prefer the fallback — the fallback's own hazard is worse. A
//! `[` at the start of a line inside a multi-line string would read as a table header to the text
//! scanner here, and a parser Codex wrote cannot be fooled that way by a file Codex wrote.
//!
//! ## What is written
//!
//! Over stdio, which is the default and what should be preferred: the path to this very
//! `unluminous-cli`, and the arguments that make it a server. There is no port, nothing is listening,
//! and the process lives for exactly as long as the agent's conversation.
//!
//! ```json
//! { "type": "stdio", "command": "C:\\Program Files\\Unluminous\\unluminous-cli.exe", "args": ["mcp", "serve"] }
//! ```
//!
//! Over HTTP, for somebody who has turned the endpoint on in `Settings -> Tools -> MCP`:
//!
//! ```json
//! { "type": "http", "url": "http://127.0.0.1:7345/mcp" }
//! ```

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::mcp::{endpoint, Transport, DEFAULT_PORT};

/// What a server is called in an agent's configuration unless somebody chose another name.
pub const DEFAULT_NAME: &str = "unluminous";

/// Which agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    /// Claude Code: `~/.claude.json`, or `.mcp.json` at a project root.
    Claude,
    /// Codex: `~/.codex/config.toml`.
    Codex,
}

impl Client {
    pub const ALL: [Client; 2] = [Client::Claude, Client::Codex];

    /// The word the command line and the settings page spell it with.
    pub fn name(self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Codex => "codex",
        }
    }

    /// What a person calls it.
    pub fn title(self) -> &'static str {
        match self {
            Client::Claude => "Claude Code",
            Client::Codex => "Codex",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "claude" | "claude-code" | "claude code" => Some(Client::Claude),
            "codex" => Some(Client::Codex),
            _ => None,
        }
    }
}

/// Whether the server is written for every project or only for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Every project this person opens.
    #[default]
    User,
    /// This folder only. `.mcp.json` for Claude Code, which is the file a team commits.
    Project,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "user" | "global" => Some(Scope::User),
            "project" | "local" | "folder" => Some(Scope::Project),
            _ => None,
        }
    }
}

/// Everything that decides what gets written.
#[derive(Debug, Clone)]
pub struct Wanted {
    pub name: String,
    pub transport: Transport,
    pub port: u16,
    pub scope: Scope,
    /// The program to launch, for stdio. This `unluminous-cli`, unless a caller knows better.
    pub program: PathBuf,
    /// The folder a project-scoped install is for.
    pub folder: PathBuf,
}

impl Default for Wanted {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME.to_owned(),
            transport: Transport::default(),
            port: DEFAULT_PORT,
            scope: Scope::default(),
            program: unluminous_cli_program(),
            folder: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl Wanted {
    /// The entry an `mcpServers` block holds, which is the shape Claude Code, Claude Desktop and
    /// most other clients read.
    pub fn entry(&self) -> Value {
        match self.transport {
            Transport::Stdio => json!({
                "type": "stdio",
                "command": self.program.to_string_lossy(),
                "args": ["mcp", "serve"],
                "env": {},
            }),
            // A `url` with no `type` is read as a stdio server and skipped, which is a mistake
            // worth not making for somebody who copies this block.
            Transport::Http => json!({ "type": "http", "url": endpoint(self.port) }),
        }
    }

    /// The `[mcp_servers.<name>]` table Codex reads.
    pub fn toml_table(&self) -> String {
        let mut out = format!("[mcp_servers.{}]\n", self.name);
        match self.transport {
            Transport::Stdio => {
                out.push_str(&format!("command = {}\n", toml_string(&self.program.to_string_lossy())));
                out.push_str("args = [\"mcp\", \"serve\"]\n");
            }
            Transport::Http => {
                out.push_str(&format!("url = {}\n", toml_string(&endpoint(self.port))));
            }
        }
        out
    }

    /// The whole block somebody pastes into a client that has no button of its own.
    ///
    /// The JSON is laid out here rather than by `to_string_pretty`, which puts every element of an
    /// array on a line of its own and turns a two word `args` into four lines. What this is for is
    /// being read on a settings page and pasted into a file, so it is laid out the way somebody
    /// would write it.
    pub fn example(&self, client: Client) -> String {
        match client {
            Client::Claude => {
                let inner = match self.transport {
                    Transport::Stdio => format!(
                        "      \"type\": \"stdio\",
      \"command\": {},
      \"args\": [\"mcp\", \"serve\"]",
                        json_string(&self.program.to_string_lossy())
                    ),
                    Transport::Http => format!(
                        "      \"type\": \"http\",
      \"url\": {}",
                        json_string(&endpoint(self.port))
                    ),
                };
                format!(
                    "{{
  \"mcpServers\": {{
    {}: {{
{inner}
    }}
  }}
}}",
                    json_string(&self.name)
                )
            }
            Client::Codex => self.toml_table(),
        }
    }
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    /// A sentence for a person, which is also what the task comment and the settings page show.
    pub message: String,
    /// The file it ended up in.
    pub file: PathBuf,
    /// True when the agent's own command did it, false when the file was edited directly.
    pub through_the_cli: bool,
}

/// Put Unluminous into a client's configuration.
pub fn install(client: Client, wanted: &Wanted) -> Result<Done, String> {
    match client {
        Client::Claude => claude_install(wanted),
        Client::Codex => codex_install(wanted),
    }
}

/// Take it out again.
pub fn remove(client: Client, wanted: &Wanted) -> Result<Done, String> {
    match client {
        Client::Claude => claude_remove(wanted),
        Client::Codex => codex_remove(wanted),
    }
}

/// Whether a client already has a server by this name, and what it points at.
///
/// Read from the file rather than from the client's own command, because this is asked every time
/// the settings page is opened and starting a process to answer it would be a page that hesitates.
pub fn installed(client: Client, wanted: &Wanted) -> Option<Value> {
    match client {
        Client::Claude => {
            let file = claude_file(wanted);
            let value: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
            value.get("mcpServers")?.get(&wanted.name).cloned()
        }
        Client::Codex => {
            let text = std::fs::read_to_string(codex_file()).ok()?;
            let table = table_of(&text, &format!("mcp_servers.{}", wanted.name))?;
            Some(json!({ "toml": table }))
        }
    }
}

// ------------------------------------------------------------------------------------ Claude Code

/// Where Claude Code keeps this scope's servers.
pub fn claude_file(wanted: &Wanted) -> PathBuf {
    match wanted.scope {
        Scope::User => home().join(".claude.json"),
        Scope::Project => wanted.folder.join(".mcp.json"),
    }
}

fn claude_install(wanted: &Wanted) -> Result<Done, String> {
    let entry = wanted.entry();
    let file = claude_file(wanted);
    if let Some(done) = through_claude_cli(wanted, &entry, &file) {
        return Ok(done);
    }
    write_json_server(&file, &wanted.name, Some(&entry))?;
    Ok(Done {
        message: format!(
            "Wrote `{}` into {}. Restart Claude Code and `/mcp` will list it.",
            wanted.name,
            file.display()
        ),
        file,
        through_the_cli: false,
    })
}

/// `claude mcp add-json`, which is the supported way in and does its own locking.
fn through_claude_cli(wanted: &Wanted, entry: &Value, file: &Path) -> Option<Done> {
    let scope = match wanted.scope {
        Scope::User => "user",
        Scope::Project => "project",
    };
    // Removed first, so installing twice is a change rather than a refusal. A name that is not
    // there is not an error worth reporting, which is why the outcome is ignored.
    run(&program_named("claude"), &["mcp", "remove", &wanted.name, "--scope", scope], &wanted.folder);
    let added = run(
        &program_named("claude"),
        &["mcp", "add-json", &wanted.name, &entry.to_string(), "--scope", scope],
        &wanted.folder,
    )?;
    added.then(|| Done {
        message: format!(
            "`claude mcp add-json {} --scope {scope}` wrote it. Restart Claude Code and `/mcp` will list it.",
            wanted.name
        ),
        file: file.to_path_buf(),
        through_the_cli: true,
    })
}

fn claude_remove(wanted: &Wanted) -> Result<Done, String> {
    let file = claude_file(wanted);
    let scope = match wanted.scope {
        Scope::User => "user",
        Scope::Project => "project",
    };
    if run(&program_named("claude"), &["mcp", "remove", &wanted.name, "--scope", scope], &wanted.folder)
        == Some(true)
    {
        return Ok(Done {
            message: format!("`claude mcp remove {}` took it out.", wanted.name),
            file,
            through_the_cli: true,
        });
    }
    write_json_server(&file, &wanted.name, None)?;
    Ok(Done {
        message: format!("Took `{}` out of {}.", wanted.name, file.display()),
        file,
        through_the_cli: false,
    })
}

/// Set or clear one server in a file holding an `mcpServers` object.
///
/// Everything else in the file is read, kept and written back, because for user scope that file is
/// the whole of somebody's Claude Code settings. A copy is taken first, and the copy is what makes
/// this recoverable if anything about the file was not what was expected.
fn write_json_server(file: &Path, name: &str, entry: Option<&Value>) -> Result<(), String> {
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    let mut value: Value = if existing.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&existing)
            .map_err(|problem| format!("{} is not readable as JSON: {problem}", file.display()))?
    };
    if !value.is_object() {
        return Err(format!("{} does not hold a JSON object.", file.display()));
    }
    if !existing.is_empty() {
        let backup = file.with_extension("unluminous-backup");
        std::fs::write(&backup, &existing)
            .map_err(|problem| format!("could not write {}: {problem}", backup.display()))?;
    }
    let servers = value
        .as_object_mut()
        .expect("an object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    let servers = servers.as_object_mut().expect("an object");
    match entry {
        Some(entry) => {
            servers.insert(name.to_owned(), entry.clone());
        }
        None => {
            servers.remove(name);
        }
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).ok();
    }
    let written = serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("could not lay the file out: {problem}"))?;
    std::fs::write(file, written)
        .map_err(|problem| format!("could not write {}: {problem}", file.display()))
}

// ------------------------------------------------------------------------------------------ Codex

/// Where Codex keeps its configuration. It has one file for everything.
pub fn codex_file() -> PathBuf {
    home().join(".codex").join("config.toml")
}

fn codex_install(wanted: &Wanted) -> Result<Done, String> {
    let file = codex_file();
    if let Some(done) = through_codex_cli(wanted, &file) {
        return Ok(done);
    }
    write_toml_table(&file, &format!("mcp_servers.{}", wanted.name), Some(&wanted.toml_table()))?;
    Ok(Done {
        message: format!(
            "Wrote `[mcp_servers.{}]` into {}. Restart Codex and it will be there.",
            wanted.name,
            file.display()
        ),
        file,
        through_the_cli: false,
    })
}

fn through_codex_cli(wanted: &Wanted, file: &Path) -> Option<Done> {
    // Removed first, so installing twice is a change rather than a refusal.
    run(&program_named("codex"), &["mcp", "remove", &wanted.name], &wanted.folder);
    let program = wanted.program.to_string_lossy().into_owned();
    let url = endpoint(wanted.port);
    // Codex names an HTTP server by its url and a stdio one by the command after `--`, which is the
    // same separator Claude Code uses and for the same reason: everything after it belongs to the
    // server rather than to the agent.
    let arguments: Vec<&str> = match wanted.transport {
        Transport::Stdio => vec!["mcp", "add", &wanted.name, "--", &program, "mcp", "serve"],
        Transport::Http => vec!["mcp", "add", &wanted.name, "--url", &url],
    };
    let added = run(&program_named("codex"), &arguments, &wanted.folder)?;
    added.then(|| Done {
        message: format!(
            "`codex mcp add {}` wrote it. Restart Codex and it will be there.",
            wanted.name
        ),
        file: file.to_path_buf(),
        through_the_cli: true,
    })
}

fn codex_remove(wanted: &Wanted) -> Result<Done, String> {
    let file = codex_file();
    if run(&program_named("codex"), &["mcp", "remove", &wanted.name], &wanted.folder) == Some(true) {
        return Ok(Done {
            message: format!("`codex mcp remove {}` took it out.", wanted.name),
            file,
            through_the_cli: true,
        });
    }
    write_toml_table(&file, &format!("mcp_servers.{}", wanted.name), None)?;
    Ok(Done {
        message: format!("Took `[mcp_servers.{}]` out of {}.", wanted.name, file.display()),
        file,
        through_the_cli: false,
    })
}

/// Put one table into a TOML file, or take it out, leaving every other line exactly as it was.
///
/// Written as text rather than through a parser, and that is deliberate. The file holds comments, a
/// `notify` array, a table for every project that has been trusted, and a marketplace entry with a
/// Windows extended path in it. A parse-and-serialise round trip would give all of that back
/// reformatted with the comments gone, which is a worse thing to do to somebody's configuration
/// than not writing to it at all. One table is found by its header, replaced up to the next header,
/// and everything else is untouched.
fn write_toml_table(file: &Path, table: &str, contents: Option<&str>) -> Result<(), String> {
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    if !existing.is_empty() {
        let backup = file.with_extension("toml.unluminous-backup");
        std::fs::write(&backup, &existing)
            .map_err(|problem| format!("could not write {}: {problem}", backup.display()))?;
    }
    let without = remove_table(&existing, table);
    let mut written = without;
    if let Some(contents) = contents {
        if !written.is_empty() && !written.ends_with('\n') {
            written.push('\n');
        }
        if !written.is_empty() {
            written.push('\n');
        }
        written.push_str(contents);
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).ok();
    }
    std::fs::write(file, written)
        .map_err(|problem| format!("could not write {}: {problem}", file.display()))
}

/// The text with one `[table]` and everything under it taken out.
fn remove_table(text: &str, table: &str) -> String {
    let header = format!("[{table}]");
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = trimmed == header;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    // Whatever blank lines the removed table left behind go with it, so installing and removing
    // twice does not grow the file a line at a time.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// One `[table]` and the lines under it, if the text has one.
fn table_of(text: &str, table: &str) -> Option<String> {
    let header = format!("[{table}]");
    let mut out: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if trimmed == header {
                out = Some(String::new());
                continue;
            }
            if out.is_some() {
                break;
            }
        }
        if let Some(collected) = out.as_mut() {
            collected.push_str(line);
            collected.push('\n');
        }
    }
    out.map(|collected| format!("{header}\n{collected}").trim_end().to_owned())
}

// ------------------------------------------------------------------------------------- the pieces

/// A JSON string, for the laid out example. `serde_json` is what escapes it, so a path with a
/// backslash or a quotation mark in it comes out the way a parser will read it back.
fn json_string(value: &str) -> String {
    Value::String(value.to_owned()).to_string()
}

/// A TOML basic string. Only two characters need escaping in a Windows path, and both are here.
fn toml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The `unluminous-cli` an agent should be told to launch.
///
/// An absolute path rather than the bare name, because an agent's subprocess does not inherit the
/// person's shell and may have no useful `PATH` at all — which is the single most common reason an
/// MCP server shows as failed to connect.
///
/// It is worked out rather than assumed to be this process, because the two programs that ask for
/// it are `unluminous-cli` itself and **the window**, and `current_exe` in the window is `unluminous.exe`.
/// So: this program when this program is already it, then the one beside it — an installed Unluminous
/// puts them in the same folder and a build puts them in the same `target` folder, which is right
/// with nothing configured — and `UNLUMINOUS_CLI_BIN` overrides both. It is the mirror of
/// `client::unluminous_program`, which finds the window from the client.
pub fn unluminous_cli_program() -> PathBuf {
    let name = if cfg!(windows) { "unluminous-cli.exe" } else { "unluminous-cli" };
    if let Some(named) = std::env::var_os("UNLUMINOUS_CLI_BIN") {
        return PathBuf::from(named);
    }
    let Ok(running) = std::env::current_exe() else {
        return PathBuf::from(name);
    };
    if running.file_stem().map(|stem| stem == "unluminous-cli").unwrap_or(false) {
        return running;
    }
    if let Some(beside) = running.parent().map(|folder| folder.join(name)) {
        if beside.is_file() {
            return beside;
        }
    }
    PathBuf::from(name)
}

fn program_named(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

/// Run an agent's own command. `None` when the program is not there at all, which is the case the
/// fallback is for; `Some(false)` when it ran and would not do it.
fn run(program: &str, arguments: &[&str], folder: &Path) -> Option<bool> {
    let mut attempt = std::process::Command::new(program);
    attempt.args(arguments).current_dir(folder);
    // Nothing it prints is wanted: what matters is whether it worked, and the sentence a person
    // reads is written here so that both roads say the same thing.
    attempt.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    match attempt.status() {
        Ok(status) => Some(status.success()),
        Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => {
            // On Windows a `.cmd` shim is what npm installs, and a bare name is what a real
            // executable is. Try the other spelling before giving up.
            if let Some(bare) = program.strip_suffix(".cmd") {
                let mut second = std::process::Command::new(bare);
                second.args(arguments).current_dir(folder);
                second.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                return second.status().ok().map(|status| status.success());
            }
            None
        }
        Err(_) => None,
    }
}

/// The person's home folder. `HOME` first, then Windows's own pair, which is the order every other
/// program on both platforms reads them in.
fn home() -> PathBuf {
    if let Some(home) = std::env::var_os("UNLUMINOUS_HOME") {
        // Named so a test can install into a folder of its own rather than into the real settings
        // of the person running it — the rule `Store::at` already keeps for Unluminous's own settings.
        return PathBuf::from(home);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile);
    }
    match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
        (Some(drive), Some(path)) => PathBuf::from(drive).join(path),
        _ => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test at a time, in a home of its own. Where the configuration goes is named by an
    /// environment variable, which belongs to the whole process rather than to one test — the same
    /// reason `services::control`'s tests take turns.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Home {
        folder: PathBuf,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Home {
        fn drop(&mut self) {
            std::env::remove_var("UNLUMINOUS_HOME");
            std::fs::remove_dir_all(&self.folder).ok();
        }
    }

    fn a_home(name: &str) -> Home {
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let folder = std::env::temp_dir().join(format!("unluminous-mcp-install-{name}"));
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::env::set_var("UNLUMINOUS_HOME", &folder);
        Home { folder, _turn: turn }
    }

    /// What the installers write when the agent's own command is not there, which is what the file
    /// half of this module is for and the only half a test can drive.
    fn wanted(home: &Home) -> Wanted {
        Wanted {
            program: PathBuf::from("C:\\Program Files\\Unluminous\\unluminous-cli.exe"),
            folder: home.folder.clone(),
            ..Wanted::default()
        }
    }

    #[test]
    fn the_stdio_entry_names_this_program_and_carries_no_port() {
        let entry = Wanted { program: PathBuf::from("/opt/unluminous/unluminous-cli"), ..Wanted::default() }
            .entry();
        assert_eq!(entry["type"], json!("stdio"));
        assert_eq!(entry["command"], json!("/opt/unluminous/unluminous-cli"));
        assert_eq!(entry["args"], json!(["mcp", "serve"]));
        assert!(entry.get("url").is_none(), "stdio has no url");
    }

    #[test]
    fn the_http_entry_carries_a_type_because_a_url_without_one_is_skipped() {
        // Claude Code reads an entry with a `url` and no `type` as a stdio server and refuses it.
        let entry = Wanted { transport: Transport::Http, port: 7345, ..Wanted::default() }.entry();
        assert_eq!(entry["type"], json!("http"));
        assert_eq!(entry["url"], json!("http://127.0.0.1:7345/mcp"));
    }

    #[test]
    fn a_windows_path_survives_being_written_as_toml() {
        let table = Wanted {
            program: PathBuf::from("C:\\Program Files\\Unluminous\\unluminous-cli.exe"),
            ..Wanted::default()
        }
        .toml_table();
        assert!(table.contains("[mcp_servers.unluminous]"), "{table}");
        assert!(
            table.contains(r#"command = "C:\\Program Files\\Unluminous\\unluminous-cli.exe""#),
            "a backslash has to be escaped or the path is read as an escape: {table}"
        );
    }

    #[test]
    fn claude_scope_decides_which_file_and_a_project_install_is_the_committed_one() {
        let home = a_home("claude-scope");
        let user = claude_file(&Wanted { scope: Scope::User, ..wanted(&home) });
        assert_eq!(user, home.folder.join(".claude.json"));
        let project = claude_file(&Wanted { scope: Scope::Project, ..wanted(&home) });
        assert_eq!(project, home.folder.join(".mcp.json"));
    }

    #[test]
    fn installing_into_claude_keeps_everything_else_in_the_file() {
        let home = a_home("claude-keeps");
        let file = home.folder.join(".claude.json");
        std::fs::write(
            &file,
            r#"{"numStartups":42,"mcpServers":{"ghidra":{"type":"stdio","command":"python"}}}"#,
        )
        .expect("write");
        write_json_server(&file, "unluminous", Some(&wanted(&home).entry())).expect("it writes");
        let back: Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert_eq!(back["numStartups"], json!(42), "the rest of the settings must survive");
        assert_eq!(back["mcpServers"]["ghidra"]["command"], json!("python"));
        assert_eq!(back["mcpServers"]["unluminous"]["args"], json!(["mcp", "serve"]));
        assert!(
            file.with_extension("unluminous-backup").is_file(),
            "a copy is taken before somebody's settings are rewritten"
        );
    }

    #[test]
    fn installing_into_claude_twice_leaves_one_entry_and_removing_leaves_none() {
        let home = a_home("claude-twice");
        let file = home.folder.join(".claude.json");
        let wanted = wanted(&home);
        write_json_server(&file, "unluminous", Some(&wanted.entry())).expect("once");
        write_json_server(&file, "unluminous", Some(&wanted.entry())).expect("twice");
        assert!(installed(Client::Claude, &wanted).is_some());
        write_json_server(&file, "unluminous", None).expect("removed");
        assert!(installed(Client::Claude, &wanted).is_none());
        let back: Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert!(back["mcpServers"].is_object(), "the block stays, empty");
    }

    #[test]
    fn a_claude_file_that_is_not_json_is_refused_rather_than_overwritten() {
        let home = a_home("claude-broken");
        let file = home.folder.join(".claude.json");
        std::fs::write(&file, "this is not json").expect("write");
        let problem = write_json_server(&file, "unluminous", Some(&wanted(&home).entry()))
            .expect_err("it should refuse");
        assert!(problem.contains("not readable as JSON"), "{problem}");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "this is not json");
    }

    #[test]
    fn installing_into_codex_leaves_every_other_table_and_every_comment_alone() {
        let home = a_home("codex-keeps");
        let file = home.folder.join(".codex").join("config.toml");
        std::fs::create_dir_all(file.parent().expect("a folder")).expect("make it");
        let before = "# how this machine is set up\nnotify = [\"a\"]\n\n[projects.'c:\\jason']\ntrust_level = \"trusted\"\n";
        std::fs::write(&file, before).expect("write");
        write_toml_table(&file, "mcp_servers.unluminous", Some(&wanted(&home).toml_table()))
            .expect("it writes");
        let after = std::fs::read_to_string(&file).expect("read");
        assert!(after.contains("# how this machine is set up"), "the comment must survive: {after}");
        assert!(after.contains("[projects.'c:\\jason']"), "{after}");
        assert!(after.contains("[mcp_servers.unluminous]"), "{after}");
        assert!(after.contains("args = [\"mcp\", \"serve\"]"), "{after}");
    }

    #[test]
    fn installing_into_codex_twice_leaves_one_table_and_removing_leaves_none() {
        let home = a_home("codex-twice");
        let file = home.folder.join(".codex").join("config.toml");
        std::fs::create_dir_all(file.parent().expect("a folder")).expect("make it");
        std::fs::write(&file, "notify = [\"a\"]\n").expect("write");
        let wanted = wanted(&home);
        for _ in 0..2 {
            write_toml_table(&file, "mcp_servers.unluminous", Some(&wanted.toml_table())).expect("write");
        }
        let after = std::fs::read_to_string(&file).expect("read");
        assert_eq!(after.matches("[mcp_servers.unluminous]").count(), 1, "{after}");
        assert!(installed(Client::Codex, &wanted).is_some());
        write_toml_table(&file, "mcp_servers.unluminous", None).expect("remove");
        let after = std::fs::read_to_string(&file).expect("read");
        assert!(!after.contains("[mcp_servers.unluminous]"), "{after}");
        assert!(after.contains("notify"), "the rest survives: {after}");
        assert!(installed(Client::Codex, &wanted).is_none());
    }

    #[test]
    fn a_table_is_replaced_rather_than_the_one_after_it_being_eaten() {
        let text = "[a]\nx = 1\n\n[mcp_servers.unluminous]\ncommand = \"old\"\n\n[b]\ny = 2\n";
        let without = remove_table(text, "mcp_servers.unluminous");
        assert!(without.contains("[a]"), "{without}");
        assert!(without.contains("[b]"), "{without}");
        assert!(without.contains("y = 2"), "{without}");
        assert!(!without.contains("command = \"old\""), "{without}");
    }

    #[test]
    fn the_copyable_example_is_what_each_client_actually_reads() {
        let wanted = Wanted { program: PathBuf::from("/opt/unluminous-cli"), ..Wanted::default() };
        let claude: Value =
            serde_json::from_str(&wanted.example(Client::Claude)).expect("valid JSON");
        assert_eq!(claude["mcpServers"]["unluminous"]["type"], json!("stdio"));
        let codex = wanted.example(Client::Codex);
        assert!(codex.starts_with("[mcp_servers.unluminous]"), "{codex}");
    }

    #[test]
    fn the_program_an_agent_is_told_to_launch_can_be_named() {
        // The window asks for this too, and `current_exe` there is `unluminous.exe` rather than the
        // client, so it has to be worked out rather than assumed.
        let previous = std::env::var_os("UNLUMINOUS_CLI_BIN");
        std::env::set_var("UNLUMINOUS_CLI_BIN", "/somewhere/else/unluminous-cli");
        assert_eq!(unluminous_cli_program(), PathBuf::from("/somewhere/else/unluminous-cli"));
        std::env::remove_var("UNLUMINOUS_CLI_BIN");
        // Running inside the client's own test binary, the answer is not the test binary: it is the
        // client beside it, or the bare name when there is not one.
        let worked_out = unluminous_cli_program();
        assert!(
            worked_out.file_stem().map(|stem| stem == "unluminous-cli").unwrap_or(false),
            "it should name unluminous-cli, not {}",
            worked_out.display()
        );
        if let Some(value) = previous {
            std::env::set_var("UNLUMINOUS_CLI_BIN", value);
        }
    }

    #[test]
    fn a_client_and_a_scope_are_spelled_the_same_everywhere() {
        assert_eq!(Client::parse("Claude Code"), Some(Client::Claude));
        assert_eq!(Client::parse("codex"), Some(Client::Codex));
        assert_eq!(Client::parse("cursor"), None);
        assert_eq!(Scope::parse("project"), Some(Scope::Project));
        assert_eq!(Scope::default().name(), "user");
    }
}
