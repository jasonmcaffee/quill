//! Which debug adapter a language uses, where it lives on this machine, and how a run configuration
//! becomes that adapter's own launch request.
//!
//! **Which debugger a language uses is data in the plugin, and the debugger itself is code here.**
//! That is the rule `language.renders` and `run.project` already follow, made a third time:
//! `services::plugins::DEBUGGERS` is what a manifest may name, and this file is what each name knows
//! how to do. So the most a third-party manifest can do is name an adapter that shipped in the
//! binary, visibly, and nothing in a plugin is executed.
//!
//! **Nothing is ever fetched.** Zed downloads its adapters; Quill does not, because the rule that
//! keeps a document from making a network request keeps the editor from making one too. Each entry
//! knows the programs it will look for on `PATH`, in order, and a settings key overrides the lot
//! (`debug.lldb = C:\tools\codelldb\adapter\codelldb.exe`, the `terminal.shell` pattern: empty means
//! "what this machine has"). Pressing Debug with no adapter on the machine is not an error dialog and
//! not a dead button — it is one sentence in the status bar saying what was looked for and where it
//! comes from, built from the entry that knew.
//!
//! ## What each adapter is
//!
//! **`lldb`** — Rust and native code. `codelldb` first, then `lldb-dap`. CodeLLDB carries Rust-aware
//! formatters and is the better answer where it is installed; `lldb-dap` ships inside every LLVM
//! distribution and is the floor. One honesty that belongs on the page rather than hidden: with the
//! MSVC toolchain rustup installs by default on Windows, LLDB reads PDB debug information
//! incompletely — breakpoints and stepping work, and some enums and collections render poorly. The
//! variables tree shows what the adapter says rather than pretending.
//!
//! **`node`** — JavaScript and TypeScript. Microsoft's js-debug, run as
//! `node <path>/dapDebugServer.js <port>` and connected to on localhost. It is a server rather than
//! a stdio child, which is why `AdapterCommand` has both shapes. It debugs Node programs, and
//! TypeScript through source maps — the same programs `run.file = node {file}` and `npx tsx {file}`
//! already run.
//!
//! ## Two things measured about CodeLLDB 1.12.3, so they are not rediscovered
//!
//! **It reads `repl` as an LLDB command line**, not as an expression: it says so itself on the
//! console — `Console is in 'commands' mode, prefix expressions with '?'` — and `evaluate` with that
//! context answers `'total' is not a valid command`. So Quill's `Evaluate Expression` asks in the
//! `watch` context, which is what its name says it does. `app::debug::DebugState::evaluate` records
//! it beside the call.
//!
//! **An expression that does not resolve ends the session.** Evaluating a name that is not in scope
//! gets a Python traceback and, on this machine, takes the debuggee with it — with nothing said on
//! the protocol channel about why. It is CodeLLDB's fault rather than Quill's, and what Quill does
//! about it is what it should: the session ends, `debug status` says so, and starting another works
//! at once. `debug output` is where the adapter's own words go, which is the only place any of this
//! is visible at all — and the reason the adapter's standard error is read rather than swallowed.
//!
//! **What about Python?** Quill ships no Python plugin, so there is no `python` entry — an entry no
//! manifest can name would be dead code. The day a Python plugin is written, `debugpy`
//! (`python -m debugpy.adapter`, stdio, and it *is* the protocol natively) is the entry to add.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use quill_dap::AdapterCommand;

use crate::services::run_configurations::{split_command, Configuration};

/// The port a server-shaped adapter is asked to listen on.
///
/// One fixed port rather than one the operating system chose, because js-debug is told the port on
/// its command line and there is no way to ask it afterwards which it took. High enough to be well
/// clear of anything a person's own project is likely to be serving on.
pub const SERVER_PORT: u16 = 8123;

/// One debugger Quill knows how to drive.
pub struct Debugger {
    /// The name a manifest's `debug.adapter` gives, and the settings key `debug.<name>`.
    pub name: &'static str,
    /// The programs looked for on `PATH`, in the order they are preferred.
    pub programs: &'static [&'static str],
    /// Where a person gets it, which is the second half of the refusal sentence. "Install it" with
    /// no idea where from is not a message worth writing.
    pub comes_from: &'static str,
    /// What a person should know before believing what they are shown. Empty for an adapter with
    /// nothing to warn about.
    pub caveat: &'static str,
}

/// Every debugger built into this version of Quill.
///
/// The list is checked against `plugins::DEBUGGERS` by a test, so a name a manifest may give and a
/// name this file knows how to start cannot come apart.
pub const ALL: &[Debugger] = &[
    Debugger {
        name: "lldb",
        // CodeLLDB first: it carries Rust-aware formatters, so a `Vec` reads as a list of its
        // elements rather than as a pointer and two integers.
        programs: &["codelldb", "lldb-dap"],
        comes_from: "lldb-dap ships with LLVM, and codelldb is the CodeLLDB extension's adapter",
        caveat: "With the MSVC toolchain, LLDB reads PDB debug information incompletely: breakpoints and stepping work, and some enums and collections render poorly.",
    },
    Debugger {
        name: "node",
        // js-debug is a JavaScript program rather than an executable, so what is looked for is the
        // thing that runs it. Which copy of js-debug to run is the settings key's job.
        programs: &["node"],
        comes_from: "js-debug is Microsoft's Node debugger; set debug.node to its dapDebugServer.js",
        caveat: "",
    },
];

/// The entry called `name`, if this version has one.
pub fn find(name: &str) -> Option<&'static Debugger> {
    ALL.iter().find(|entry| entry.name == name)
}

/// Why a session could not be started.
///
/// A type rather than a string, because the two reasons want different sentences and one of them is
/// about the configuration rather than about the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No adapter of that name is on this machine.
    NotInstalled { name: String, looked_for: Vec<String>, comes_from: String },
    /// The configuration runs a build tool rather than a program, so debugging it would debug the
    /// build tool.
    BuildTool { program: String, advice: String },
    /// The configuration has no command in it at all.
    NoCommand(String),
}

impl Refusal {
    /// The one sentence the status bar shows.
    ///
    /// Built from the entry that knew, so it names what was looked for and where it comes from
    /// rather than saying "no debugger found" — which tells a person nothing they can act on.
    pub fn message(&self) -> String {
        match self {
            Refusal::NotInstalled { name, looked_for, comes_from } => format!(
                "Debugging with {name} needs {} on PATH. {comes_from}.",
                either(looked_for)
            ),
            Refusal::BuildTool { program, advice } => {
                format!("{program} builds the program rather than being it. {advice}")
            }
            Refusal::NoCommand(name) => format!("{name} has no command to debug."),
        }
    }
}

/// What starting a debugger needs: the adapter to run, and the `launch` request's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub adapter: AdapterCommand,
    pub body: Value,
    /// What the entry warns about, shown once when the session starts. Empty for an adapter with
    /// nothing to say.
    pub caveat: &'static str,
}

/// Work out how to debug `configuration` with the adapter called `name`.
///
/// `override_path` is what the settings say, which beats everything; `root` is the project.
///
/// The whole of the machine-specific knowledge is here — where the adapter lives, what its launch
/// request is called, whether the configuration is a thing that can be debugged at all — so the
/// window asks one question and gets either something startable or a sentence.
pub fn prepare(
    name: &str,
    configuration: &Configuration,
    root: &Path,
    override_path: Option<&str>,
) -> Result<Launch, Refusal> {
    let entry = find(name).ok_or_else(|| Refusal::NotInstalled {
        name: name.to_owned(),
        looked_for: Vec::new(),
        comes_from: "this version of Quill does not know it".to_owned(),
    })?;
    let Some((program, args)) = configuration.program_and_arguments() else {
        return Err(Refusal::NoCommand(configuration.name.clone()));
    };
    let cwd = configuration.working_directory(root);
    let env = configuration.environment();
    match name {
        "lldb" => prepare_lldb(entry, program, args, cwd, env, override_path),
        "node" => prepare_node(entry, program, args, cwd, env, override_path),
        // Unreachable while `ALL` and `DEBUGGERS` agree, which a test keeps true — but a fourth
        // entry added to one list and not to this match should say so rather than start nothing.
        _ => Err(Refusal::NotInstalled {
            name: name.to_owned(),
            looked_for: Vec::new(),
            comes_from: "this version of Quill does not know how to start it".to_owned(),
        }),
    }
}

/// lldb-dap and CodeLLDB take the same launch shape: a program, its arguments, a folder and an
/// environment.
fn prepare_lldb(
    entry: &'static Debugger,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    override_path: Option<&str>,
) -> Result<Launch, Refusal> {
    // A configuration whose program is `cargo` runs a build tool, not the program, and handing
    // `cargo` to lldb would debug cargo. Rather than being cleverly wrong — deriving the artifact
    // from Cargo's JSON messages is Zed's locators and is a design of its own — this refuses with
    // the sentence that says exactly what to do instead.
    if let Some(tool) = build_tool(&program) {
        return Err(Refusal::BuildTool {
            program: tool.to_owned(),
            advice: "Debugging a Cargo project means a configuration that names the built binary, such as `target\\debug\\myapp.exe`.".to_owned(),
        });
    }
    let found = locate(entry, override_path)?;
    let adapter = AdapterCommand::stdio(found, Vec::new()).in_folder(cwd.clone());
    Ok(Launch {
        adapter,
        body: json!({
            "name": "Quill",
            "type": "lldb",
            "request": "launch",
            "program": program,
            "args": args,
            "cwd": cwd.to_string_lossy(),
            "env": environment_object(&env),
            // The one place Quill asks for the debuggee to be run in its own terminal, which is
            // what puts a real ConPTY behind it — §7.2. An adapter that cannot honour it simply
            // sends `output` events instead, which the tile also draws.
            "runInTerminal": true,
            "stopOnEntry": false,
        }),
        caveat: entry.caveat,
    })
}

/// js-debug is a server: `node <dapDebugServer.js> <port>`, then a socket on localhost.
fn prepare_node(
    entry: &'static Debugger,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    override_path: Option<&str>,
) -> Result<Launch, Refusal> {
    // The settings key names the server's own JavaScript file rather than an executable, which is
    // what js-debug ships. Without it there is nothing to start, and saying so is more use than
    // starting `node` with no script and watching it exit.
    let server = override_path.map(str::trim).filter(|path| !path.is_empty()).ok_or_else(|| {
        Refusal::NotInstalled {
            name: entry.name.to_owned(),
            looked_for: vec!["dapDebugServer.js".to_owned()],
            comes_from: entry.comes_from.to_owned(),
        }
    })?;
    let node = on_path("node").ok_or_else(|| Refusal::NotInstalled {
        name: entry.name.to_owned(),
        looked_for: vec!["node".to_owned()],
        comes_from: "Node.js".to_owned(),
    })?;
    let adapter = AdapterCommand::server(
        node,
        vec![server.to_owned(), SERVER_PORT.to_string()],
        SERVER_PORT,
    )
    .in_folder(cwd.clone());
    // js-debug names the script and its arguments separately, and it takes the program as
    // `runtimeExecutable` plus `program` — so a configuration of `node server.js --port 3000` is
    // the runtime, the script and the rest.
    let (script, rest) = match program.eq_ignore_ascii_case("node")
        || program.to_lowercase().ends_with("node.exe")
    {
        true => (args.first().cloned().unwrap_or_default(), args[1.min(args.len())..].to_vec()),
        // `npx tsx server.ts` and anything else: the whole command line is the program, and
        // js-debug runs it through its own runtime.
        false => (program.clone(), args.clone()),
    };
    Ok(Launch {
        adapter,
        body: json!({
            "name": "Quill",
            "type": "pwa-node",
            "request": "launch",
            "program": script,
            "args": rest,
            "cwd": cwd.to_string_lossy(),
            "env": environment_object(&env),
            "console": "integratedTerminal",
            // Source maps are what make debugging TypeScript through `npx tsx` work at all, which is
            // half of what this entry is for.
            "sourceMaps": true,
            "stopOnEntry": false,
        }),
        caveat: entry.caveat,
    })
}

/// The adapter's own program: what the settings say, or the first of the entry's names that is on
/// `PATH`.
fn locate(entry: &'static Debugger, override_path: Option<&str>) -> Result<PathBuf, Refusal> {
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    for program in entry.programs {
        if let Some(found) = on_path(program) {
            return Ok(found);
        }
    }
    Err(Refusal::NotInstalled {
        name: entry.name.to_owned(),
        looked_for: entry.programs.iter().map(|name| (*name).to_owned()).collect(),
        comes_from: entry.comes_from.to_owned(),
    })
}

/// Where `program` is on `PATH`, if it is there.
///
/// `PATH` is walked here rather than a `where`/`which` being run, because starting a process to find
/// out whether a process can be started is a round trip that the window would wait for and because
/// `where.exe` is not on every Windows.
pub fn on_path(program: &str) -> Option<PathBuf> {
    // A path with a separator in it was meant as a path rather than as a name to look up, which is
    // what a settings key holding a full path relies on.
    let named = Path::new(program);
    if named.components().count() > 1 {
        return named.is_file().then(|| named.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<String> = match cfg!(windows) {
        true => std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_owned())
            .split(';')
            .map(|extension| extension.trim().to_lowercase())
            .filter(|extension| !extension.is_empty())
            .collect(),
        // On Unix the name is the whole of it.
        false => Vec::new(),
    };
    for folder in std::env::split_paths(&path) {
        let plain = folder.join(program);
        if plain.is_file() {
            return Some(plain);
        }
        for extension in &extensions {
            let with = folder.join(format!("{program}{extension}"));
            if with.is_file() {
                return Some(with);
            }
        }
    }
    None
}

/// The build tools that must not be handed to a native debugger, and what they are called.
///
/// A configuration whose program is one of these runs something that produces the program rather
/// than being it. `cargo run` under lldb debugs cargo, which is a session that starts, works, and is
/// about the wrong process — the exact shape of wrongness that a refusal is better than.
fn build_tool(program: &str) -> Option<&'static str> {
    let name = Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    ["cargo", "npm", "npx", "yarn", "pnpm", "make", "dotnet", "go"]
        .into_iter()
        .find(|tool| *tool == name)
}

/// `NAME=value` pairs as the object every adapter's launch request takes.
fn environment_object(env: &[(String, String)]) -> Value {
    let mut object = serde_json::Map::new();
    for (name, value) in env {
        object.insert(name.clone(), json!(value));
    }
    Value::Object(object)
}

/// `a or b`, `a, b or c` — what the refusal names, read as a person would say it.
fn either(names: &[String]) -> String {
    match names {
        [] => "an adapter".to_owned(),
        [one] => one.clone(),
        [first @ .., last] => format!("{} or {last}", first.join(", ")),
    }
}

/// The command line a `run.file` template makes, which is what `Debug Current File` debugs.
///
/// Here rather than in the window so that running a file and debugging it split the template the
/// same way — `run_configurations::split_command`'s rules, one reading.
pub fn command_for_file(template: &str, path: &Path) -> String {
    template.replace(
        crate::services::run_configurations::FILE_PLACEHOLDER,
        &path.to_string_lossy(),
    )
}

/// True when a command line could be debugged at all, which is what dims `Debug` rather than
/// removing it.
///
/// The same question [`prepare`] asks, asked without building anything, so the menu and the action
/// cannot disagree about which configurations offer a Debug entry.
pub fn can_debug(name: &str, command: &str) -> bool {
    let Some(program) = split_command(command).into_iter().next() else {
        return false;
    };
    match name {
        // A native debugger needs the built binary rather than the tool that builds it.
        "lldb" => build_tool(&program).is_none(),
        // js-debug runs the command through its own runtime, so a build tool is an ordinary thing
        // for it to be pointed at: `npx tsx server.ts` is exactly what it is for.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::DEBUGGERS;

    fn configuration(command: &str) -> Configuration {
        Configuration::new("Test", command)
    }

    /// The two lists cannot come apart: a name a manifest may give is a name this file knows how to
    /// start, and the other way round.
    #[test]
    fn every_name_a_manifest_may_give_is_one_this_file_can_start() {
        for name in DEBUGGERS {
            assert!(find(name).is_some(), "{name} is offered to manifests but has no entry");
        }
        for entry in ALL {
            assert!(
                DEBUGGERS.contains(&entry.name),
                "{} has an entry but no manifest could ever name it",
                entry.name
            );
        }
    }

    #[test]
    fn every_entry_says_where_it_comes_from() {
        for entry in ALL {
            assert!(!entry.comes_from.is_empty(), "{} has no second half to its refusal", entry.name);
            assert!(!entry.programs.is_empty(), "{} looks for nothing", entry.name);
        }
    }

    /// The refusal names what was looked for and where it comes from. "No debugger found" tells a
    /// person nothing they can act on.
    #[test]
    fn a_missing_adapter_is_a_sentence_naming_what_was_looked_for() {
        let refusal = Refusal::NotInstalled {
            name: "lldb".to_owned(),
            looked_for: vec!["codelldb".to_owned(), "lldb-dap".to_owned()],
            comes_from: "lldb-dap ships with LLVM".to_owned(),
        };
        let said = refusal.message();
        assert!(said.contains("codelldb or lldb-dap"), "{said}");
        assert!(said.contains("ships with LLVM"), "{said}");
        assert!(said.starts_with("Debugging with lldb needs"), "{said}");
    }

    #[test]
    fn one_name_is_named_on_its_own_and_three_read_as_a_list() {
        assert_eq!(either(&["node".to_owned()]), "node");
        assert_eq!(either(&["a".to_owned(), "b".to_owned()]), "a or b");
        assert_eq!(
            either(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
            "a, b or c"
        );
    }

    /// Rather than being cleverly wrong, `cargo run` under lldb is refused with the sentence that
    /// says what to do instead. Deriving the binary from Cargo's own JSON messages is the right
    /// eventual answer and is a design of its own.
    #[test]
    fn handing_cargo_to_a_native_debugger_is_refused_with_advice() {
        let refused = prepare("lldb", &configuration("cargo run"), Path::new("."), None)
            .expect_err("cargo builds the program rather than being it");
        let Refusal::BuildTool { program, .. } = &refused else { panic!("{refused:?}") };
        assert_eq!(program, "cargo");
        let said = refused.message();
        assert!(said.contains("target\\debug\\myapp.exe"), "{said}");
        assert!(!can_debug("lldb", "cargo run"));
        assert!(!can_debug("lldb", "npm run build"));
        assert!(can_debug("lldb", "target\\debug\\app.exe --fast"));
    }

    /// And the other way for node, because js-debug runs the command through its own runtime —
    /// `npx tsx server.ts` is exactly what that entry is for.
    #[test]
    fn a_build_tool_is_an_ordinary_thing_to_point_js_debug_at() {
        assert!(can_debug("node", "npx tsx server.ts"));
        assert!(can_debug("node", "node server.js"));
        assert!(!can_debug("node", "   "), "and nothing at all is still nothing");
    }

    #[test]
    fn a_configuration_with_no_command_says_so_rather_than_starting_something() {
        let refused = prepare("lldb", &configuration("   "), Path::new("."), None)
            .expect_err("nothing to run");
        assert_eq!(refused, Refusal::NoCommand("Test".to_owned()));
        assert!(refused.message().contains("no command to debug"));
    }

    #[test]
    fn a_name_this_version_does_not_know_is_refused_rather_than_started() {
        let refused = prepare("gdb", &configuration("app.exe"), Path::new("."), None)
            .expect_err("no such entry");
        assert!(refused.message().contains("gdb"), "{}", refused.message());
    }

    /// The settings key beats the walk of `PATH`, which is what makes a machine keeping its adapter
    /// somewhere odd work at all.
    #[test]
    fn the_settings_path_is_used_as_it_was_written() {
        let prepared = prepare(
            "lldb",
            &configuration("target\\debug\\app.exe --fast"),
            Path::new("C:\\project"),
            Some("C:\\tools\\codelldb\\adapter\\codelldb.exe"),
        )
        .expect("a path that was given rather than looked for");
        assert_eq!(
            prepared.adapter.program.as_deref(),
            Some(Path::new("C:\\tools\\codelldb\\adapter\\codelldb.exe"))
        );
        assert_eq!(prepared.body["program"], "target\\debug\\app.exe");
        assert_eq!(prepared.body["args"], json!(["--fast"]));
        assert_eq!(prepared.body["request"], "launch");
        assert_eq!(prepared.body["runInTerminal"], true);
        assert!(!prepared.caveat.is_empty(), "the MSVC PDB limit reaches the person");
    }

    #[test]
    fn the_configurations_folder_and_environment_reach_the_launch_request() {
        let mut configuration = configuration("app.exe");
        configuration.directory = "backend".to_owned();
        configuration.env = "PORT=3000; DEBUG=app:*".to_owned();
        let prepared =
            prepare("lldb", &configuration, Path::new("C:\\project"), Some("lldb-dap.exe"))
                .expect("prepared");
        assert!(
            prepared.body["cwd"].as_str().expect("a folder").ends_with("backend"),
            "{}",
            prepared.body["cwd"]
        );
        assert_eq!(prepared.body["env"]["PORT"], "3000");
        assert_eq!(prepared.body["env"]["DEBUG"], "app:*");
    }

    /// js-debug is a JavaScript file rather than an executable, so an adapter that has not been
    /// pointed at one says so rather than starting `node` with nothing to run.
    #[test]
    fn node_with_no_server_named_is_refused_with_the_name_of_what_is_missing() {
        let refused = prepare("node", &configuration("node server.js"), Path::new("."), None)
            .expect_err("no dapDebugServer.js");
        let said = refused.message();
        assert!(said.contains("dapDebugServer.js"), "{said}");
        assert!(said.contains("js-debug"), "{said}");
    }

    #[test]
    fn a_file_template_becomes_a_command_line_with_the_path_in_it() {
        assert_eq!(
            command_for_file("node {file}", Path::new("C:\\p\\a.js")),
            "node C:\\p\\a.js"
        );
        assert_eq!(
            command_for_file("npx tsx {file}", Path::new("C:\\p\\a.ts")),
            "npx tsx C:\\p\\a.ts"
        );
    }

    /// A name with a separator in it was meant as a path, which is what a settings key holding a
    /// full path relies on — and a path that is not there is not silently looked up on `PATH`.
    #[test]
    fn a_name_that_is_a_path_is_not_looked_for_on_the_path() {
        assert!(on_path("C:\\definitely\\not\\here\\lldb-dap.exe").is_none());
        assert!(on_path("quill-no-such-program-anywhere").is_none());
    }

    /// The one thing that must be true of `PATH` walking on this machine, asserted against a program
    /// every machine that can build Quill has.
    #[test]
    fn a_program_that_is_really_on_the_path_is_found() {
        let found = on_path("cargo").expect("cargo is on the PATH of a machine building Quill");
        assert!(found.is_file(), "{}", found.display());
    }
}
