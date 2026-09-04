//! Which debug adapter a language uses, where it lives on this machine, and how a run configuration
//! becomes that adapter's own launch request.
//!
//! **Which debugger a language uses is data in the plugin, and the debugger itself is code here.**
//! That is the rule `language.renders` and `run.project` already follow, made a third time:
//! `services::plugins::DEBUGGERS` is what a manifest may name, and this file is what each name knows
//! how to do. So the most a third-party manifest can do is name an adapter that shipped in the
//! binary, visibly, and nothing in a plugin is executed.
//!
//! **Nothing is ever fetched.** Zed downloads its adapters; Unluminate does not, because the rule that
//! keeps a document from making a network request keeps the editor from making one too. Each entry
//! knows the programs it will look for on `PATH`, in order, and a settings key overrides the lot
//! (`debug.lldb = C:\tools\codelldb\adapter\codelldb.exe`, the `terminal.shell` pattern: empty means
//! "what this machine has"). Pressing Debug with no adapter on the machine is not an error dialog and
//! not a dead button — it is one sentence in the status bar saying what was looked for and where it
//! comes from, built from the entry that knew.
//!
//! ## `PATH`, and then where installers really put things (`task-1692`)
//!
//! Looking on `PATH` alone told machines that *have* a debugger that they have none, which is the
//! commoner failure of the two on Windows: LLVM's own installer offers not to add itself to `PATH`,
//! and CodeLLDB lives inside a VS Code extension folder that never was on it. So [`well_known`] is
//! searched after `PATH` and before giving up — the VS Code family's extension folders, LLVM's two
//! install locations, Visual Studio's bundled LLVM, homebrew, Xcode, and the versioned `lldb-dap-20`
//! names Debian ships. It is reading directories on this machine; nothing here is a network request.
//! It is also what makes the `node` entry work without `debug.node` being set, which it needed
//! before: js-debug's server is a folder like any other once something has unpacked it.
//!
//! ## An adapter that is really missing says how to get one
//!
//! Naming what was looked for without saying where to get it is what sent an agent off to download a
//! debugger by hand, which is `task-1692`'s first sentence. Every entry therefore carries an
//! [`Install`] — one command a platform — which reaches the person three ways: in the refusal
//! sentence, as the debug tile's Install button, and as `unluminate-cli debug adapters` and
//! `debug install`. Pressing it **runs that command in the run tile**, a visible terminal with a
//! program in it, which is the distinction `task-1687` §13 drew and this keeps: **the editor never
//! reaches out**; a command the person pressed a button for, where they can watch it, does. It is the
//! move `tools/release.ps1` already makes when it installs `gh` with winget.
//!
//! On Windows that command is [`CODELLDB_FETCH`], which unpacks CodeLLDB's own `.vsix` into
//! `%LOCALAPPDATA%\Unluminate\adapters` — no elevation, fifty megabytes rather than LLVM's two and a half
//! gigabytes, and the adapter this file prefers anyway. `tools/get-debug-adapter.ps1` is the same
//! thing as a script, and since `task-1692` its output is found by [`well_known`] rather than needing
//! a `debug.lldb` line pasted in afterwards — which was a step in the middle of a two-step install
//! that nobody should have had to know about.
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
//! context answers `'total' is not a valid command`. So Unluminate's `Evaluate Expression` asks in the
//! `watch` context, which is what its name says it does. `app::debug::DebugState::evaluate` records
//! it beside the call.
//!
//! **An expression that does not resolve ends the session.** Evaluating a name that is not in scope
//! gets a Python traceback and, on this machine, takes the debuggee with it — with nothing said on
//! the protocol channel about why. It is CodeLLDB's fault rather than Unluminate's, and what Unluminate does
//! about it is what it should: the session ends, `debug status` says so, and starting another works
//! at once. `debug output` is where the adapter's own words go, which is the only place any of this
//! is visible at all — and the reason the adapter's standard error is read rather than swallowed.
//!
//! **What about Python?** Unluminate ships no Python plugin, so there is no `python` entry — an entry no
//! manifest can name would be dead code. The day a Python plugin is written, `debugpy`
//! (`python -m debugpy.adapter`, stdio, and it *is* the protocol natively) is the entry to add.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use unluminate_dap::AdapterCommand;

use crate::services::run_configurations::{split_command, Configuration};

/// The port a server-shaped adapter is asked to listen on.
///
/// One fixed port rather than one the operating system chose, because js-debug is told the port on
/// its command line and there is no way to ask it afterwards which it took. High enough to be well
/// clear of anything a person's own project is likely to be serving on.
pub const SERVER_PORT: u16 = 8123;

/// One debugger Unluminate knows how to drive.
pub struct Debugger {
    /// The name a manifest's `debug.adapter` gives, and the settings key `debug.<name>`.
    pub name: &'static str,
    /// The programs looked for on `PATH`, in the order they are preferred.
    pub programs: &'static [&'static str],
    /// Where a person gets it, which is the second half of the refusal sentence. "Install it" with
    /// no idea where from is not a message worth writing.
    pub comes_from: &'static str,
    /// The command that installs it on this machine, which is the third half — the one that turns a
    /// refusal into something a person or an agent can act on without leaving Unluminate.
    pub install: Install,
    /// The VS Code extension this adapter ships inside, when it ships inside one and there is no
    /// better way to get it.
    ///
    /// The **fallback** to [`Self::install`], and empty for both entries as things stand: CodeLLDB
    /// and js-debug are both fetched as release assets, which needs no editor installed at all.
    pub extension: &'static str,
    /// What a person should know before believing what they are shown. Empty for an adapter with
    /// nothing to warn about.
    pub caveat: &'static str,
}

/// The command that installs an adapter, one a platform.
///
/// Three fields rather than one, because a machine can only run its own and a message that offered
/// all three would be a message a person has to read past. [`Install::here`] picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Install {
    pub windows: &'static str,
    pub macos: &'static str,
    pub linux: &'static str,
}

impl Install {
    /// The command for the machine Unluminate is running on.
    pub fn here(&self) -> &'static str {
        match () {
            _ if cfg!(windows) => self.windows,
            _ if cfg!(target_os = "macos") => self.macos,
            _ => self.linux,
        }
    }
}

impl Debugger {
    /// The command that installs this adapter on **this** machine, given what it already has.
    ///
    /// The entry's own command first — for `lldb` on Windows that is [`CODELLDB_FETCH`], which needs
    /// no editor, no elevation and no package manager — and an editor's extension only where there
    /// is no other way, which is `node`: js-debug publishes no standalone installer at all. An empty
    /// answer means Unluminate has nothing to offer here, and the refusal then says what the settings key
    /// is for rather than pretending.
    ///
    /// **`code` is a `.cmd` on Windows**, which `CreateProcess` will not run, so the command names
    /// `cmd /c` in front of it — written out where it can be seen, which is the rule
    /// `run_configurations::split_command` already makes: no shell runs a command line unless the
    /// command line says so itself.
    pub fn install_command(&self) -> String {
        let mine = self.install.here();
        if !mine.is_empty() {
            return mine.to_owned();
        }
        if !self.extension.is_empty() {
            if let Some(code) = on_path("code") {
                return match cfg!(windows) {
                    true => format!("cmd /c code --install-extension {}", self.extension),
                    false => format!("{} --install-extension {}", code.display(), self.extension),
                };
            }
        }
        String::new()
    }
}

/// The command that fetches CodeLLDB and unpacks it where [`well_known`] will find it.
///
/// One PowerShell command rather than a script, because the Install button has to work in an
/// installed Unluminate that has no source checkout beside it — and because a person reading it in the
/// status bar can see exactly what it will do before pressing anything. **Unluminate does not run this by
/// itself**: it is offered, and pressing Install puts it in the run tile as a visible program.
///
/// The version is pinned rather than `latest`, so two installs a year apart install the same thing —
/// `tools/get-debug-adapter.ps1`'s own choice, kept.
pub const CODELLDB_FETCH: &str = concat!(
    "powershell -NoProfile -Command \"",
    "$ErrorActionPreference='Stop'; ",
    r"$into=Join-Path $env:LOCALAPPDATA 'Unluminate\adapters'; ",
    "New-Item -ItemType Directory -Force $into | Out-Null; ",
    "$zip=Join-Path $env:TEMP 'codelldb.vsix'; ",
    "Invoke-WebRequest -Uri https://github.com/vadimcn/codelldb/releases/download/v1.12.3/codelldb-x86_64-windows.vsix -OutFile $zip; ",
    "Expand-Archive -Force $zip (Join-Path $into 'codelldb'); ",
    "Remove-Item $zip; ",
    "Write-Host 'CodeLLDB is in' $into\"",
);

/// The command that fetches js-debug's standalone DAP server and unpacks it where [`well_known`]
/// will find it.
///
/// `tar` rather than `Expand-Archive`, because the asset is a `.tar.gz` and every Windows since 10
/// ships bsdtar. **Named by its full path**, because a machine with Git for Windows on its `PATH` has
/// a different `tar` in front of it — measured here: that one answered
/// `gzip: stdin: unexpected end of file` on an archive Windows' own tar unpacked without complaint.
///
/// The version is pinned for [`CODELLDB_FETCH`]'s reason: two installs a year apart install the same
/// thing.
pub const JS_DEBUG_FETCH: &str = concat!(
    "powershell -NoProfile -Command \"",
    "$ErrorActionPreference='Stop'; ",
    r"$into=Join-Path $env:LOCALAPPDATA 'Unluminate\adapters'; ",
    "New-Item -ItemType Directory -Force $into | Out-Null; ",
    "$tar=Join-Path $env:TEMP 'js-debug.tar.gz'; ",
    "Invoke-WebRequest -Uri https://github.com/microsoft/vscode-js-debug/releases/download/v1.105.0/js-debug-dap-v1.105.0.tar.gz -OutFile $tar; ",
    r"$unpack=Join-Path $env:SystemRoot 'System32\tar.exe'; & $unpack -xzf $tar -C $into; ",
    "Remove-Item $tar; ",
    "Write-Host 'js-debug is in' $into\"",
);

/// The same for macOS and Linux, where `curl` and `tar` are both on `PATH` and there is no
/// PowerShell to spell it in.
///
/// No quotes inside it, on purpose: `run_configurations::split_command` splits the way a shell splits
/// a double-quoted word, so a nested quote would end the argument early. The one path it names is
/// under the home directory, where nothing has a space in it.
pub const JS_DEBUG_FETCH_UNIX: &str = concat!(
    "sh -c \"",
    "set -e; into=$HOME/.local/share/unluminate/adapters; mkdir -p $into; ",
    "curl -fsSL https://github.com/microsoft/vscode-js-debug/releases/download/v1.105.0/js-debug-dap-v1.105.0.tar.gz ",
    "| tar -xz -C $into; ",
    "echo js-debug is in $into\"",
);

/// Every debugger built into this version of Unluminate.
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
        // On Windows, CodeLLDB's own `.vsix` unpacked into Unluminate's adapters folder: it is a zip
        // holding `adapter/codelldb.exe` and the LLDB it drives, so unpacking it is the whole
        // install — no elevation, fifty megabytes rather than LLVM's two and a half gigabytes, and
        // the adapter this entry prefers anyway. It is what `tools/get-debug-adapter.ps1` does,
        // written out as one command so an installed Unluminate with no source checkout can offer it.
        // `winget install --id LLVM.LLVM -e` is the other answer and needs elevation.
        install: Install {
            windows: CODELLDB_FETCH,
            macos: "brew install llvm",
            linux: "sudo apt install lldb",
        },
        extension: "vadimcn.vscode-lldb",
        caveat: "With the MSVC toolchain, LLDB reads PDB debug information incompletely: breakpoints and stepping work, and some enums and collections render poorly.",
    },
    Debugger {
        name: "node",
        // js-debug is a JavaScript program rather than an executable, so what is looked for is the
        // thing that runs it. Which copy of js-debug to run is the settings key's job.
        programs: &["node"],
        comes_from: "js-debug is Microsoft's Node debugger, and its standalone DAP server is a release of vscode-js-debug",
        // js-debug publishes no standalone installer, so the command that gets one is the editor's
        // own extension install — the same folder `well_known` then finds it in. Somebody with no
        // VS Code sets `debug.node` to a `dapDebugServer.js` of their own, which the refusal says.
        // js-debug's **standalone DAP server**, which is a release asset of its own rather than the
        // extension. Measured on this machine: `code --install-extension ms-vscode.js-debug`
        // answers "already installed" — it is one of VS Code's built-in extensions — and the copy it
        // means carries no `dapDebugServer.js` at all, because VS Code runs js-debug in process. So
        // installing the extension is not a way to get the thing Unluminate needs, and the release asset
        // is.
        install: Install {
            windows: JS_DEBUG_FETCH,
            macos: JS_DEBUG_FETCH_UNIX,
            linux: JS_DEBUG_FETCH_UNIX,
        },
        extension: "",
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
    /// No adapter of that name is on this machine. `install` is the command that would get one,
    /// empty when this version of Unluminate does not know the adapter at all and so cannot say.
    NotInstalled {
        name: String,
        looked_for: Vec<String>,
        comes_from: String,
        install: String,
    },
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
            // The install command is the half `task-1692` added: naming what was looked for without
            // saying where to get it is what makes a person go and find out for themselves.
            Refusal::NotInstalled { name, looked_for, comes_from, install } => {
                let said = format!(
                    "Debugging with {name} needs {}. {comes_from}.",
                    either(looked_for)
                );
                match install.is_empty() {
                    true => said,
                    false => format!("{said} Install it with: {install}"),
                }
            }
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
/// `built` is what a locator produced — the binary `cargo build` said it made, and the debuggee's
/// own arguments — and when it is there it stands **in place of** the configuration's command line.
/// That is the one thing that makes `cargo run` debuggable at all (`services::locators`), and it is
/// a parameter rather than a second function because everything else about the launch is the same.
///
/// The whole of the machine-specific knowledge is here — where the adapter lives, what its launch
/// request is called, whether the configuration is a thing that can be debugged at all — so the
/// window asks one question and gets either something startable or a sentence.
pub fn prepare(
    name: &str,
    configuration: &Configuration,
    root: &Path,
    override_path: Option<&str>,
    built: Option<(String, Vec<String>)>,
) -> Result<Launch, Refusal> {
    let entry = find(name).ok_or_else(|| Refusal::NotInstalled {
        name: name.to_owned(),
        looked_for: Vec::new(),
        comes_from: "this version of Unluminate does not know it".to_owned(),
        install: String::new(),
    })?;
    let Some((program, args)) = built.or_else(|| configuration.program_and_arguments()) else {
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
            comes_from: "this version of Unluminate does not know how to start it".to_owned(),
            install: String::new(),
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
            // `cargo run` and `cargo test` no longer arrive here at all — `services::locators` asks
            // cargo what it built and hands the answer in as `built`. What still arrives is a cargo
            // subcommand that produces no program, and the advice says which two do.
            advice: "Unluminate debugs `cargo run` and `cargo test` by building them first; anything else needs a configuration that names the built binary, such as `target\\debug\\myapp.exe`.".to_owned(),
        });
    }
    let found = locate(entry, override_path)?;
    let adapter = AdapterCommand::stdio(found, Vec::new()).in_folder(cwd.clone());
    Ok(Launch {
        adapter,
        body: json!({
            "name": "Unluminate",
            "type": "lldb",
            "request": "launch",
            "program": program,
            "args": args,
            "cwd": cwd.to_string_lossy(),
            "env": environment_object(&env),
            // The one place Unluminate asks for the debuggee to be run in its own terminal, which is
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
    // what js-debug ships — and when it is not set, `well_known` looks inside the extension folders
    // where the editors keep it. Before `task-1692` this was the settings key or nothing, which
    // meant an adapter that would not start until it had been configured.
    let server = override_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(js_debug)
        .ok_or_else(|| Refusal::NotInstalled {
            name: entry.name.to_owned(),
            looked_for: vec!["dapDebugServer.js".to_owned()],
            comes_from: entry.comes_from.to_owned(),
            install: entry.install_command(),
        })?;
    let node = on_path("node").ok_or_else(|| Refusal::NotInstalled {
        name: entry.name.to_owned(),
        looked_for: vec!["node".to_owned()],
        comes_from: "Node.js".to_owned(),
        install: match cfg!(windows) {
            true => "winget install --id OpenJS.NodeJS -e".to_owned(),
            false => "brew install node".to_owned(),
        },
    })?;
    let adapter = AdapterCommand::server(
        node,
        vec![server.to_string_lossy().to_string(), SERVER_PORT.to_string()],
        SERVER_PORT,
    )
    .in_folder(cwd.clone());
    // js-debug names the script and its arguments separately, and it takes the program as
    // `runtimeExecutable` plus `program` — so a configuration of `node server.js --port 3000` is
    // the runtime, the script and the rest.
    // Asked of the program's **name**, not of the whole word. `run add` writes the runtime as the
    // path it found, which under nvm is `~/.nvm/versions/node/v22.14.0/bin/node` — that is neither
    // `node` nor something ending in `node.exe`, so the split never happened and js-debug was handed
    // the node binary as the JavaScript file to debug.
    let (script, rest) = match program_name(&program) == "node" {
        true => (args.first().cloned().unwrap_or_default(), args[1.min(args.len())..].to_vec()),
        // `npx tsx server.ts` and anything else: the whole command line is the program, and
        // js-debug runs it through its own runtime.
        false => (program.clone(), args.clone()),
    };
    Ok(Launch {
        adapter,
        body: json!({
            "name": "Unluminate",
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

/// The adapter's own program: what the settings say, the first of the entry's names that is on
/// `PATH`, or the first that is where an installer would have put it.
///
/// The order is the order of confidence. A path in the settings was written by a person and is used
/// as it was written; `PATH` is the machine's own answer; [`well_known`] is Unluminate guessing, correctly
/// and often, at the places the installers really use.
pub fn locate(entry: &'static Debugger, override_path: Option<&str>) -> Result<PathBuf, Refusal> {
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    for program in entry.programs {
        if let Some(found) = on_path(program) {
            return Ok(found);
        }
    }
    if let Some(found) = well_known(entry.name) {
        return Ok(found);
    }
    Err(Refusal::NotInstalled {
        name: entry.name.to_owned(),
        looked_for: entry.programs.iter().map(|name| (*name).to_owned()).collect(),
        comes_from: entry.comes_from.to_owned(),
        install: entry.install_command(),
    })
}

/// Where an adapter of this name is on a machine that has one but has not put it on `PATH`.
///
/// One function with a match rather than a list of patterns on each entry, because two of the three
/// answers are not patterns at all — js-debug is a JavaScript file inside an editor's extension, and
/// Debian's lldb-dap is a versioned name rather than a versioned folder. What they share is that
/// none of it is executed and none of it is fetched: these are directories being read.
pub fn well_known(adapter: &str) -> Option<PathBuf> {
    match adapter {
        "lldb" => codelldb().or_else(lldb_dap),
        "node" => js_debug(),
        _ => None,
    }
}

/// CodeLLDB, which on most machines that have any LLDB at all is inside a VS Code extension folder.
///
/// Preferred over `lldb-dap` for the reason [`ALL`] prefers it: the Rust formatters.
fn codelldb() -> Option<PathBuf> {
    let adapter = match cfg!(windows) {
        true => "codelldb.exe",
        false => "codelldb",
    };
    // Unluminate's own folder first, which is where `tools/get-debug-adapter.ps1` and the Install button
    // unpack the `.vsix`. Before `task-1692` that folder was found only by a `debug.lldb` line
    // somebody pasted in by hand, which is a step in the middle of a two-step install that nobody
    // should have to know about.
    if let Some(own) = unluminate_adapters()
        .map(|folder| folder.join("codelldb").join("extension").join("adapter").join(adapter))
        .filter(|path| path.is_file())
    {
        return Some(own);
    }
    extension_folders()
        .into_iter()
        .filter_map(|folder| newest_child(&folder, "vadimcn.vscode-lldb-"))
        .map(|extension| extension.join("adapter").join(adapter))
        .find(|path| path.is_file())
}

/// Where Unluminate keeps an adapter it unpacked for itself.
///
/// Under the local application data, which needs no elevation and is a folder a person can delete —
/// the choice `tools/get-debug-adapter.ps1` already made, read here rather than restated.
pub fn unluminate_adapters() -> Option<PathBuf> {
    match cfg!(windows) {
        true => std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Unluminate").join("adapters")),
        false => home_folder().map(|home| home.join(".local").join("share").join("unluminate").join("adapters")),
    }
}

/// `lldb-dap`, in the places the LLVM distributions put it.
fn lldb_dap() -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        folders.push(PathBuf::from(r"C:\Program Files\LLVM\bin"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            folders.push(PathBuf::from(local).join(r"Programs\LLVM\bin"));
        }
        // Visual Studio's own LLVM, which is two globbed segments down: the year and the edition.
        for root in [r"C:\Program Files\Microsoft Visual Studio", r"C:\Program Files (x86)\Microsoft Visual Studio"] {
            for year in children(Path::new(root)) {
                for edition in children(&year) {
                    folders.push(edition.join(r"VC\Tools\Llvm\x64\bin"));
                }
            }
        }
    } else {
        folders.push(PathBuf::from("/opt/homebrew/opt/llvm/bin"));
        folders.push(PathBuf::from("/usr/local/opt/llvm/bin"));
        folders.push(PathBuf::from("/Applications/Xcode.app/Contents/Developer/usr/bin"));
        folders.push(PathBuf::from("/usr/bin"));
        folders.push(PathBuf::from("/usr/local/bin"));
        // Debian and Ubuntu ship LLVM in a folder a version wide, newest first.
        if let Some(newest) = newest_child(Path::new("/usr/lib"), "llvm-") {
            folders.push(newest.join("bin"));
        }
    }
    let program = match cfg!(windows) {
        true => "lldb-dap.exe",
        false => "lldb-dap",
    };
    for folder in &folders {
        let named = folder.join(program);
        if named.is_file() {
            return Some(named);
        }
    }
    // Debian and Ubuntu also carry the version in the program's own name, and the newest of those is
    // the one to prefer — hence the descending walk rather than a fixed list.
    if !cfg!(windows) {
        for folder in &folders {
            if let Some(versioned) = newest_child(folder, "lldb-dap-").filter(|path| path.is_file()) {
                return Some(versioned);
            }
        }
    }
    None
}

/// js-debug's own server, inside Microsoft's extension.
///
/// This is what makes `debug.node` optional, which it was not before `task-1692`: an adapter that
/// needed a settings key before it would start at all is an adapter nobody starts.
fn js_debug() -> Option<PathBuf> {
    // Unluminate's own folder first, which is where the release asset unpacks to.
    if let Some(own) = unluminate_adapters()
        .map(|folder| folder.join("js-debug").join("src").join("dapDebugServer.js"))
        .filter(|path| path.is_file())
    {
        return Some(own);
    }
    extension_folders()
        .into_iter()
        .flat_map(|folder| {
            // The nightly build is a folder of its own, and somebody who has both meant to have the
            // stable one — so it is looked at second.
            [
                newest_child(&folder, "ms-vscode.js-debug-"),
                newest_child(&folder, "ms-vscode.js-debug-nightly-"),
            ]
        })
        .flatten()
        .map(|extension| extension.join("src").join("dapDebugServer.js"))
        .find(|path| path.is_file())
}

/// Every extension folder of the VS Code family on this machine.
///
/// Six of them, because a person's editor is as likely to be Cursor or Windsurf as VS Code itself
/// and all of them keep extensions in the same shape of folder under the home directory.
fn extension_folders() -> Vec<PathBuf> {
    let Some(home) = home_folder() else {
        return Vec::new();
    };
    [".vscode", ".vscode-insiders", ".vscode-oss", ".vscode-server", ".cursor", ".windsurf"]
        .into_iter()
        .map(|editor| home.join(editor).join("extensions"))
        .filter(|folder| folder.is_dir())
        .collect()
}

/// The person's home directory, by whichever name this operating system gives it.
fn home_folder() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

/// Everything directly inside a folder, or nothing when it cannot be read.
fn children(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries.flatten().map(|entry| entry.path()).collect()
}

/// The child of `folder` whose name begins with `prefix` and whose version is the highest.
///
/// **Sorted as versions rather than as text**, because `1.11.4` sorts before `1.9.0` as a string and
/// the answer would then be a year-old CodeLLDB on a machine that has both. [`version_key`] is the
/// comparison, and it is the whole reason this is a function rather than a `find`.
fn newest_child(folder: &Path, prefix: &str) -> Option<PathBuf> {
    let mut matched: Vec<(Vec<u64>, PathBuf)> = children(folder)
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().to_string();
            let rest = name.strip_prefix(prefix)?;
            // `ms-vscode.js-debug-` is a prefix of `ms-vscode.js-debug-nightly-1.2.3`, and a
            // nightly is not a version of the stable extension. A version begins with a digit.
            rest.starts_with(|first: char| first.is_ascii_digit()).then(|| (version_key(rest), path))
        })
        .collect();
    matched.sort_by(|left, right| left.0.cmp(&right.0));
    matched.pop().map(|(_, path)| path)
}

/// A version read as the numbers in it, so `1.11.4` is above `1.9.0`.
///
/// Anything that is not a number is a separator, which is enough for every shape of version any of
/// these adapters uses and does not need a crate.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// Where `program` is on the `PATH` Unluminate searches, if it is there.
///
/// The walking is `services::login_shell::find`, so the adapter search and the Agent-Tasks board look
/// on one `PATH` rather than two that agree today. That module is also why the `PATH` it walks is not
/// simply this process's own: an Unluminate started from the Finder has four folders on it and none of them
/// is one anybody installs a program into.
pub fn on_path(program: &str) -> Option<PathBuf> {
    crate::services::login_shell::find(program)
}

/// The build tools that must not be handed to a native debugger, and what they are called.
///
/// A configuration whose program is one of these runs something that produces the program rather
/// than being it. `cargo run` under lldb debugs cargo, which is a session that starts, works, and is
/// about the wrong process — the exact shape of wrongness that a refusal is better than.
fn build_tool(program: &str) -> Option<&'static str> {
    let name = program_name(program);
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

/// Which debugger a command line wants, read from the command line itself.
///
/// **The configuration is what is being debugged, so the configuration is what is asked.** Before
/// `task-1692` the adapter came from whichever file happened to be showing, so debugging a Node
/// server while reading `README.md` answered "this file's language has not said which debugger to
/// use" — a refusal about the wrong thing entirely. The open file is still the fallback, in
/// `UnluminateApp::adapter_for`, for a command line none of this recognises.
///
/// `None` is not a failure; it means "nothing here says", and the caller asks the plugins next.
pub fn adapter_for(program: &str) -> Option<&'static str> {
    let path = Path::new(last_part(program));
    let stem = program_name(program);
    // A build tool names its debugger even though it is not the program: cargo builds native code,
    // and everything in the npm family is run by js-debug's own runtime.
    match stem.as_str() {
        "cargo" => return Some("lldb"),
        "node" | "npm" | "npx" | "yarn" | "pnpm" | "bun" | "deno" | "tsx" | "ts-node" => {
            return Some("node")
        }
        _ => {}
    }
    let extension =
        path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => Some("node"),
        "exe" => Some("lldb"),
        // A path with no extension is a built program on every operating system that has no
        // extensions; a bare *word* with no extension is a program on `PATH` whose language nothing
        // here knows, so it is left for the plugins to answer.
        "" if names_a_folder(program) => Some("lldb"),
        _ => None,
    }
}

/// The last part of a program, whichever separator it was written with.
///
/// `Path` splits on the separator of the machine it is running on, so a command line naming
/// `C:\Program Files\nodejs\node.exe` is one long file name on macOS and its stem is
/// `C:\Program Files\nodejs\node`, which matches nothing — `adapter_for` answered `lldb` for Node on
/// every machine but Windows, and `the_command_line_says_which_debugger_it_wants` failed there.
/// A configuration is text a person typed or a path `run add` wrote, and either machine can read
/// either spelling, so both separators are split here and the answer is the same everywhere.
fn last_part(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

/// The name of the program a command line runs: its last part, without the extension, folded to
/// lower case so a comparison against a word is one comparison.
fn program_name(program: &str) -> String {
    Path::new(last_part(program))
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// True when the program is a path rather than a bare word on `PATH`.
fn names_a_folder(program: &str) -> bool {
    program.contains('/') || program.contains('\\')
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
        // A native debugger needs the built binary rather than the tool that builds it — unless a
        // locator can ask the tool which binary that is, which is what makes `cargo run` debuggable.
        "lldb" => build_tool(&program).is_none() || crate::services::locators::locate(command).is_some(),
        // js-debug runs the command through its own runtime, so a build tool is an ordinary thing
        // for it to be pointed at: `npx tsx server.ts` is exactly what it is for.
        _ => true,
    }
}

/// What `unluminate-cli debug adapters` says about one debugger, and what the debug tile's own panel
/// draws when there is nothing to draw.
///
/// A type rather than a printed table, because the one command an agent runs to find out whether it
/// can debug at all should answer in fields rather than in prose. `languages` is filled in by the
/// window, which is the half that knows which plugins are switched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub name: &'static str,
    /// Where the adapter is, or nothing when this machine has none.
    pub found: Option<PathBuf>,
    /// True when `found` came from the settings rather than from a search.
    pub configured: bool,
    /// The names that were looked for, in the order they are preferred.
    pub programs: Vec<&'static str>,
    /// The languages whose files this debugger debugs, as the plugins that are on say.
    pub languages: Vec<String>,
    pub comes_from: &'static str,
    /// The command that installs it on this machine, empty when Unluminate has nothing to offer.
    pub install: String,
    /// The settings key that names it by hand.
    pub settings_key: String,
    pub caveat: &'static str,
}

impl Report {
    /// True when a session with this adapter could start right now.
    pub fn is_found(&self) -> bool {
        self.found.is_some()
    }
}

/// What is known about the adapter called `name` on this machine.
///
/// `override_path` is the `debug.<name>` setting. A setting that names something that is not there is
/// reported as **not found** rather than as configured, because saying "found" of a path that does
/// not exist is the one answer a doctor must never give.
pub fn report(entry: &'static Debugger, override_path: Option<&str>) -> Report {
    let configured = override_path.map(str::trim).is_some_and(|path| !path.is_empty());
    // What the entry actually needs, which for `node` is js-debug's own server rather than the
    // `node` that runs it — reporting "found: node.exe" of a machine with no js-debug would be a
    // doctor saying the patient is well.
    let found = match entry.name {
        "node" => override_path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(js_debug),
        _ => locate(entry, override_path).ok(),
    }
    .filter(|path| path.is_file());
    let programs = match entry.name {
        "node" => vec!["dapDebugServer.js"],
        _ => entry.programs.to_vec(),
    };
    Report {
        name: entry.name,
        configured: configured && found.is_some(),
        found,
        programs,
        languages: Vec::new(),
        comes_from: entry.comes_from,
        install: entry.install_command(),
        settings_key: format!("debug.{}", entry.name),
        caveat: entry.caveat,
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
            // And the third half, which is `task-1692`: a refusal that does not say how to get one
            // is what sends a person off to find out for themselves. Either a package manager on
            // every platform, or an extension — `node` has only the second, because js-debug
            // publishes no standalone installer at all.
            let by_hand =
                [entry.install.windows, entry.install.macos, entry.install.linux].map(str::is_empty);
            assert!(
                !entry.extension.is_empty() || by_hand.iter().all(|missing| !missing),
                "{} can be installed on some platforms and not others, which is a message nobody can write",
                entry.name
            );
        }
    }

    /// The refusal names what was looked for, where it comes from, and how to get one.
    #[test]
    fn a_missing_adapter_is_a_sentence_naming_what_was_looked_for() {
        let refusal = Refusal::NotInstalled {
            name: "lldb".to_owned(),
            looked_for: vec!["codelldb".to_owned(), "lldb-dap".to_owned()],
            comes_from: "lldb-dap ships with LLVM".to_owned(),
            install: "winget install --id LLVM.LLVM -e".to_owned(),
        };
        let said = refusal.message();
        assert!(said.contains("codelldb or lldb-dap"), "{said}");
        assert!(said.contains("ships with LLVM"), "{said}");
        assert!(said.starts_with("Debugging with lldb needs"), "{said}");
        assert!(said.contains("Install it with: winget install --id LLVM.LLVM -e"), "{said}");
    }

    /// An adapter this version does not know cannot be installed either, and says nothing rather
    /// than offering a command for a thing that does not exist.
    #[test]
    fn a_refusal_with_no_install_command_leaves_the_sentence_where_it_was() {
        let refusal = Refusal::NotInstalled {
            name: "gdb".to_owned(),
            looked_for: Vec::new(),
            comes_from: "this version of Unluminate does not know it".to_owned(),
            install: String::new(),
        };
        assert!(!refusal.message().contains("Install it with"), "{}", refusal.message());
    }

    /// Every row of the table in `task-1692` §5: the configuration says which debugger, and the open
    /// file is only what is left when it does not.
    #[test]
    fn the_command_line_says_which_debugger_it_wants() {
        assert_eq!(adapter_for("cargo"), Some("lldb"));
        assert_eq!(adapter_for("node"), Some("node"));
        assert_eq!(adapter_for("npm"), Some("node"));
        assert_eq!(adapter_for("npx"), Some("node"));
        assert_eq!(adapter_for("C:\\Program Files\\nodejs\\node.exe"), Some("node"));
        // The runtime as `run add` really writes it on a machine using nvm, which is a path several
        // folders deep and neither the word `node` nor anything ending in `node.exe`.
        assert_eq!(
            adapter_for("/Users/someone/.nvm/versions/node/v22.14.0/bin/node"),
            Some("node")
        );
        assert_eq!(adapter_for("server.js"), Some("node"));
        assert_eq!(adapter_for("src/server.ts"), Some("node"));
        assert_eq!(adapter_for("target\\debug\\unluminate.exe"), Some("lldb"));
        assert_eq!(adapter_for("./target/debug/unluminate"), Some("lldb"), "no extension is a program");
        // A bare word with no extension is a program on `PATH` whose language nothing here knows, so
        // the plugins are asked next rather than it being guessed at.
        assert_eq!(adapter_for("python"), None);
        assert_eq!(adapter_for("main.py"), None);
        assert_eq!(adapter_for(""), None);
    }

    /// `well_known` reads directories and never runs anything, so the one thing to assert about it
    /// on a machine that may have nothing installed is that it answers rather than panicking — and
    /// that anything it does find is really there.
    #[test]
    fn looking_where_the_installers_put_things_answers_a_real_file_or_nothing() {
        for adapter in ["lldb", "node"] {
            if let Some(found) = well_known(adapter) {
                assert!(found.is_file(), "{} said {}", adapter, found.display());
            }
        }
        assert_eq!(well_known("no-such-adapter"), None);
    }

    #[test]
    fn a_version_sorts_by_its_numbers_rather_than_as_text() {
        // `1.11.4` above `1.9.0` is the whole reason this exists: as text it is the other way round,
        // and the answer would be a year-old CodeLLDB on a machine that has both.
        assert!(version_key("1.11.4") > version_key("1.9.0"));
        assert!(version_key("20.0.0") > version_key("9.9.9"));
        assert_eq!(version_key("nothing-numeric"), Vec::<u64>::new());
    }

    /// The doctor's answer, on whatever this machine really has. What must be true either way is
    /// that it says something actionable: a path that exists, or a command that installs one.
    #[test]
    fn the_report_is_a_found_path_or_a_way_to_get_one() {
        for entry in ALL {
            let report = report(entry, None);
            assert_eq!(report.settings_key, format!("debug.{}", entry.name));
            assert!(!report.install.is_empty());
            match &report.found {
                Some(path) => assert!(path.is_file(), "{}", path.display()),
                None => assert!(!report.programs.is_empty(), "nothing was even looked for"),
            }
        }
    }

    /// A settings key naming something that is not there is **not** "configured": saying "found" of
    /// a path that does not exist is the one answer a doctor must never give.
    #[test]
    fn a_settings_path_that_is_not_there_is_reported_as_missing() {
        let entry = find("lldb").expect("lldb");
        let report = report(entry, Some("C:\\definitely\\not\\here\\codelldb.exe"));
        assert!(!report.is_found());
        assert!(!report.configured);
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
        let refused = prepare("lldb", &configuration("cargo run"), Path::new("."), None, None)
            .expect_err("cargo builds the program rather than being it");
        let Refusal::BuildTool { program, .. } = &refused else { panic!("{refused:?}") };
        assert_eq!(program, "cargo");
        let said = refused.message();
        assert!(said.contains("target\\debug\\myapp.exe"), "{said}");
        assert!(!can_debug("lldb", "npm run build"), "no locator asks npm what it built");
        assert!(can_debug("lldb", "target\\debug\\app.exe --fast"));
    }

    /// And `cargo run` reaches [`prepare`] as the binary rather than as cargo, which is the whole of
    /// `task-1692`'s second sentence: `services::locators` asks cargo what it built, the window hands
    /// the answer in as `built`, and the refusal above is never reached for the commonest
    /// configuration there is.
    #[test]
    fn cargo_is_debugged_as_the_binary_it_built_rather_than_being_refused() {
        assert!(can_debug("lldb", "cargo run"), "a locator can answer for cargo");
        assert!(can_debug("lldb", "cargo test --release"));
        assert!(!can_debug("lldb", "cargo fmt"), "and only where one really can");
        let built =
            Some(("C:\\p\\target\\debug\\unluminate.exe".to_owned(), vec!["--control".to_owned(), "off".to_owned()]));
        let prepared = prepare(
            "lldb",
            &configuration("cargo run -- --control off"),
            Path::new("C:\\p"),
            Some("lldb-dap.exe"),
            built,
        )
        .expect("the built binary is what is debugged");
        assert_eq!(prepared.body["program"], "C:\\p\\target\\debug\\unluminate.exe");
        assert_eq!(prepared.body["args"], json!(["--control", "off"]));
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
        let refused = prepare("lldb", &configuration("   "), Path::new("."), None, None)
            .expect_err("nothing to run");
        assert_eq!(refused, Refusal::NoCommand("Test".to_owned()));
        assert!(refused.message().contains("no command to debug"));
    }

    #[test]
    fn a_name_this_version_does_not_know_is_refused_rather_than_started() {
        let refused = prepare("gdb", &configuration("app.exe"), Path::new("."), None, None)
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
            None,
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
            prepare("lldb", &configuration, Path::new("C:\\project"), Some("lldb-dap.exe"), None)
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
    /// pointed at one — and cannot find one for itself — says which file is missing and how to get
    /// it, rather than starting `node` with nothing to run.
    ///
    /// The refusal is built rather than provoked, because whether *this* machine has js-debug is not
    /// something a test may depend on: since `task-1692` a machine that has one is found without a
    /// settings key, which is the whole point of the change.
    #[test]
    fn node_with_no_server_named_is_refused_with_the_name_of_what_is_missing() {
        let entry = find("node").expect("node");
        let refused = Refusal::NotInstalled {
            name: entry.name.to_owned(),
            looked_for: vec!["dapDebugServer.js".to_owned()],
            comes_from: entry.comes_from.to_owned(),
            install: entry.install_command(),
        };
        let said = refused.message();
        assert!(said.contains("dapDebugServer.js"), "{said}");
        assert!(said.contains("js-debug"), "{said}");
        assert!(said.contains("Install it with"), "{said}");
    }

    /// And a machine that has js-debug where the fetch put it needs no settings key at all, which is
    /// what `task-1692` changed: before it, `debug.node` was the only way the entry ever started.
    #[test]
    fn node_finds_its_own_server_when_one_has_been_unpacked() {
        let found = js_debug();
        let prepared = prepare("node", &configuration("node server.js"), Path::new("."), None, None);
        match found {
            Some(server) => {
                let launch = prepared.expect("js-debug is on this machine");
                assert_eq!(launch.adapter.args.first().map(String::as_str), Some(&*server.to_string_lossy()));
                assert_eq!(launch.body["program"], "server.js");
            }
            None => {
                let said = prepared.expect_err("nothing to start").message();
                assert!(said.contains("dapDebugServer.js"), "{said}");
            }
        }
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
        assert!(on_path("unluminate-no-such-program-anywhere").is_none());
    }

    /// The one thing that must be true of `PATH` walking on this machine, asserted against a program
    /// every machine that can build Unluminate has.
    #[test]
    fn a_program_that_is_really_on_the_path_is_found() {
        let found = on_path("cargo").expect("cargo is on the PATH of a machine building Unluminate");
        assert!(found.is_file(), "{}", found.display());
    }
}
