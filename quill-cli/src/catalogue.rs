//! Every command the CLI has, written down once.
//!
//! This is the single list. The client parses against it, `--help` is printed from it, `quill-cli
//! commands` hands it to a program or an agent as JSON, the window dispatches on the names in it,
//! and a test in this crate refuses to pass while a command in it is missing from
//! `quill-cli/docs/commands.md`. A command that is not here does not exist, and a command that is
//! here is documented.
//!
//! ## How a command is named
//!
//! `quill-cli <area> <verb>`, in that order — the noun first, as `docker container create` and
//! `dotnet tool install` are, which is the more common of the two orders and the one the .NET
//! guidance asks for: a command holding subcommands is a **grouping**, and the verb underneath it
//! is the action. Areas are what the window is made of, so somebody who can see Quill can guess the
//! area: `tab`, `pane`, `panel`, `editor`, `fold`, `terminal`, `run`, `debug`, `explorer`, `modal`,
//! `settings`, `plugins`,
//! `git`, `window`, `project`, `action`. Six commands have no area, because they are about the CLI
//! or about a whole Quill: `status`, `instances`, `launch`, `quit`, `commands` and `version`.
//!
//! Names are lower case and hyphenated — `save-as`, `go-to-file`, `find-in-files` — and never
//! abbreviated to something a reader would have to learn.
//!
//! ## It is also what an agent is given
//!
//! `mcp::tools` turns this same list into Model Context Protocol tools, so a command added here is a
//! tool an agent can call the day it is added, with this summary, these arguments and these flags.
//! That is a third reader of every line below, and the one least able to ask what was meant: write
//! the summary for somebody who cannot see the window.
//!
//! ## What a command is made of
//!
//! Positional [`Argument`]s in the order they are typed, then [`Flag`]s in any order. The client
//! turns both into one named object before sending it, so the window reads `path` without caring
//! whether the person typed it as a positional or as `--path`. That is deliberate: an agent writing
//! a command from this catalogue can always name every value with a flag and never has to count
//! positions.

use serde_json::{Map, Value};

/// One value typed after the verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argument {
    pub name: &'static str,
    pub required: bool,
    /// True when this argument swallows the rest of the line, which is what text arguments do so
    /// that `terminal send git status` needs no quotes.
    pub rest: bool,
    pub help: &'static str,
}

/// One `--name` or `--name value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    pub name: &'static str,
    /// The name of the value it takes. `None` makes it a switch that is either given or not.
    pub value: Option<&'static str>,
    pub help: &'static str,
}

/// One command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The grouping it is typed under, or `""` when it is typed on its own.
    pub area: &'static str,
    pub verb: &'static str,
    pub summary: &'static str,
    pub arguments: &'static [Argument],
    pub flags: &'static [Flag],
    pub examples: &'static [&'static str],
    /// True when the client answers it without a running Quill.
    pub local: bool,
}

impl Command {
    /// What a person types, with a space: `tab open`.
    pub fn typed(&self) -> String {
        if self.area.is_empty() {
            self.verb.to_owned()
        } else {
            format!("{} {}", self.area, self.verb)
        }
    }

    /// What goes over the wire, with a dot: `tab.open`. Two spellings of one name, because a
    /// command line is typed with spaces and a JSON key holding a space is a nuisance in every
    /// language that has to read it.
    pub fn wire(&self) -> String {
        if self.area.is_empty() {
            self.verb.to_owned()
        } else {
            format!("{}.{}", self.area, self.verb)
        }
    }

    /// The usage line: the command, then each argument, then each flag.
    pub fn usage(&self) -> String {
        let mut line = format!("quill-cli {}", self.typed());
        for argument in self.arguments {
            if argument.required {
                line.push_str(&format!(" <{}>", argument.name));
            } else {
                line.push_str(&format!(" [{}]", argument.name));
            }
        }
        for flag in self.flags {
            match flag.value {
                Some(value) => line.push_str(&format!(" [--{} <{value}>]", flag.name)),
                None => line.push_str(&format!(" [--{}]", flag.name)),
            }
        }
        line
    }

    /// The flag by this name, if the command has one.
    pub fn flag(&self, name: &str) -> Option<&'static Flag> {
        self.flags.iter().find(|flag| flag.name == name)
    }
}

/// A value's name with any leading dashes taken off.
///
/// The usage lines spell a flag `--permanent`, so that is what a caller writing a request from the
/// catalogue sends it as, and `permanent` is the name the window reads. Both are the same name and
/// this is where they are made the same name. Only the *leading* dashes go: `from-line` and
/// `wait-for` have one in the middle and it is part of the name.
pub fn argument_name(key: &str) -> &str {
    key.trim_start_matches('-')
}

/// Convert a caller's spelling of an argument to the kebab-case name in the catalogue.
///
/// Leading dashes, underscores, and word-boundary capitals are presentation differences, not
/// different values. Keeping this conversion here gives the control channel and MCP resolver one
/// rule to share.
pub fn canonical_argument_name(key: &str) -> String {
    let trimmed = argument_name(key);
    let mut canonical = String::with_capacity(trimmed.len());
    let mut previous_is_word = false;
    for character in trimmed.chars() {
        if character == '_' {
            canonical.push('-');
            previous_is_word = false;
        } else if character.is_ascii_uppercase() {
            if previous_is_word && !canonical.ends_with('-') {
                canonical.push('-');
            }
            canonical.push(character.to_ascii_lowercase());
            previous_is_word = true;
        } else {
            canonical.push(character);
            previous_is_word = character.is_ascii_lowercase() || character.is_ascii_digit();
        }
    }
    canonical
}

/// Every key with its leading dashes taken off.
///
/// A key already spelled without them wins, so a request carrying both `permanent` and
/// `--permanent` keeps the one the window would have read and does not depend on which order a JSON
/// object happened to be in.
pub fn normalise_arguments(arguments: Map<String, Value>) -> Map<String, Value> {
    if arguments.keys().all(|key| canonical_argument_name(key) == *key) {
        return arguments;
    }
    let mut out = Map::new();
    for (key, value) in &arguments {
        if canonical_argument_name(key) == *key {
            out.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in arguments {
        let name = canonical_argument_name(&key);
        if !out.contains_key(&name) {
            out.insert(name, value);
        }
    }
    out
}

/// Every name this command takes a value under: its positional arguments and its flags.
///
/// One list rather than two, because the wire has one object and the window reads a positional and a
/// flag through the same name — which is what the module's own documentation says the client's job
/// is.
pub fn value_names(command: &Command) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = command.arguments.iter().map(|value| value.name).collect();
    names.extend(command.flags.iter().map(|flag| flag.name));
    names
}

/// The keys in a request that this command has no value called.
///
/// A mistyped name is a request that did something other than what it said, and the client already
/// refuses one on the command line — "a mistyped flag quietly treated as text is a command that did
/// the wrong thing without saying so". A request that arrives over the wire from an agent has the
/// same fault and had no such answer, so this is that rule made available to the window and to the
/// MCP server. Keys are compared after [`normalise_arguments`], so a spelling with dashes is not
/// what makes one unknown.
pub fn unknown_arguments(command: &Command, arguments: &Map<String, Value>) -> Vec<String> {
    let known = value_names(command);
    arguments
        .keys()
        .map(|key| canonical_argument_name(key))
        .filter(|name| !known.contains(&name.as_str()))
        .map(|name| name)
        .collect()
}

/// The command an agent most likely meant when it puts a neighbouring command's value on this one.
///
/// These are intentionally explicit: a generic search would offer several equally plausible
/// commands for names such as `tab`, while the refusal should give one useful next step.
pub fn argument_hint(command: &Command, name: &str) -> Option<&'static str> {
    match (command.area, command.verb, name) {
        ("editor", "caret", "to-column") => Some("editor select"),
        ("editor", "caret", "tab") => Some("tab select"),
        ("editor", "select", "top") | ("editor", "complete", "top") => Some("editor scroll"),
        ("editor", "complete", "to-column") => Some("editor caret"),
        ("run", "add", "wait-for") => Some("run output"),
        ("terminal", "send", "wait-for") => Some("terminal read"),
        _ => None,
    }
}

/// Find a command by what a person typed or by what goes over the wire.
///
/// Both spellings are accepted from both sides, so `quill-cli tab.open` works and a program that
/// only has the typed name can send it. An abbreviation is not accepted: `clig.dev` asks for
/// explicit aliases rather than unique prefixes, because a prefix that is unique today stops being
/// unique when a command is added and somebody's script quietly starts doing something else.
pub fn find(name: &str) -> Option<&'static Command> {
    let wanted = name.trim().replace(' ', ".");
    let wanted = match wanted.as_str() {
        "editor.open" => "tab.open",
        "editor.reload" => "tab.reload",
        "editor.save" => "tab.save",
        "editor.close" => "tab.close",
        _ => wanted.as_str(),
    };
    COMMANDS.iter().find(|command| command.wire() == wanted)
}

/// Every area, in the order the help lists them.
pub fn areas() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for command in COMMANDS {
        if !command.area.is_empty() && !out.contains(&command.area) {
            out.push(command.area);
        }
    }
    out
}

/// The commands in one area.
pub fn in_area(area: &str) -> Vec<&'static Command> {
    COMMANDS.iter().filter(|command| command.area == area).collect()
}

/// What an area is called where it is given a heading of its own.
///
/// It lives here rather than in the thing that prints it, because two things print it now: the
/// written reference, which `examples/reference.rs` generates, and the MCP tool for the area, whose
/// title and description an agent reads instead of the reference. A second copy of these words
/// would be a second copy that falls behind, which is the same reason the commands themselves are
/// one list.
pub fn area_title(area: &'static str) -> &'static str {
    match area {
        "" => "Commands with no area",
        "window" => "window — the window itself",
        "browser" => "browser — rendered web pages in Quill tabs",
        "tab" => "tab — the files that are open",
        "pane" => "pane — the editing area split into panes",
        "editor" => "editor — safely rename symbols everywhere, find every use and definitions",
        "highlight" => "highlight — the passages marked in the project's files",
        "fold" => "fold — the blocks collapsed in the tab that is showing",
        "panel" => "panel — which edge of the window each panel is docked to",
        "terminal" => "terminal — the shells along the bottom",
        "run" => "run — the named commands the project is started with",
        "explorer" => "explorer — create, move and inspect the live project tree",
        "modal" => "modal — every dialog, driven the same way",
        "settings" => "settings — Edit -> Settings, by the names in the settings file",
        "theme" => "theme — the colours the whole window is painted in",
        "plugins" => "plugins — the languages Quill colours",
        "git" => "git — status, changed files and the Git menu",
        "action" => "action — every menu entry there is",
        "project" => "project — the folder this window is showing",
        "mcp" => "mcp — the server an AI agent drives Quill through",
        other => other,
    }
}

/// The paragraph under that heading: what the area is for, and the one thing worth knowing about it
/// before reading its commands.
pub fn area_note(area: &'static str) -> &'static str {
    match area {
        "" => "Six commands are typed on their own, because they are about the CLI or about a whole Quill rather than about one part of a window.",
        "window" => "`window screenshot` is how to see what a command did. The picture is of the real window, so it is evidence rather than a description.",
        "browser" => "Rendered tabs use the operating system browser engine. Local HTML is served from a constrained project origin, so its CSS, scripts and images work without exposing the filesystem to page JavaScript. A window renders one page at a time: every rendered tab keeps its own address, title and history, and the one view follows whichever tab is showing, so these commands act on the tab that is showing and say so when it is not a rendered one.",
        "tab" => "A tab holds a file. A relative path is resolved against the project folder, and every reply says which absolute path it used.",
        "pane" => "The editing area can be split into panes side by side, each with its own tabs, which is IntelliJ's split view. `pane split` moves the tab that is showing into a new pane on the right — it moves rather than copies, because two tabs on one file would be two documents over one path. A pane holding only that tab keeps it and the new pane opens empty, ready for the next file: opening a file always lands in the pane that has the keyboard.",
        "editor" => "Use this tool first for project-symbol work. If asked to find every place a name is used, call `references` with `name`; if asked where a name is defined, call `definition`; if asked to rename it everywhere, call `rename` with `name`, `new-name` and `apply: true`. Do not begin those jobs with grep, file search, reads or file edits. Quill's native answers combine unsaved live open tabs with the project index, distinguish code from comments and strings, and apply a role-aware project rename as one undo step per open file while safely rewriting closed files. Lines and columns count from 1.",
        "fold" => "A block that can be collapsed is a function, an `if`, a bracket that spans lines, a run of comments, an indented section, or a Markdown heading — worked out from the file itself, so nothing has to be written into it. Collapsing one hides its lines; the line numbers of everything still showing are unchanged, so `fold list` and `editor caret --line` speak the same language whatever is folded. `fold others` is the one to notice: it collapses everything that does not hold a marked passage, which is how to leave only the four places you care about on the screen.",
        "panel" => "Quill has four panels — the explorer, the terminal, the run tile and the debug tile — and each of them can be docked to any edge of the window, which is what dragging its header does. A side holds an ordered row of panels laid out left to right, so `panel dock terminal left --position 1` puts the terminal beside the explorer rather than in place of it. The terminal, run and debug tiles all draw a character grid and two grids in one strip would be two half-sized grids, so showing one puts away the other tiles **on its own side** — move one somewhere else and they are both showing at once. `panel list` says where everything is, including the rectangle each occupies, which is what to read before working out where a click lands.",
        "highlight" => "A highlight is a colour behind a passage of text. It stays there until it is cleared, in this file and next time the project is opened, and it moves with the text as the file is edited. These work on a file whether it is open or not, so `highlight apply` can mark twenty passages across twenty files in one call.",
        "terminal" => "`terminal send` types into the shell and presses Enter; `terminal read --wait-for` is how to wait for what it did. Both take `--tab` to name a tab other than the one showing, and naming a tab does not show it, so a build in one tab and a dev server in another can each be spoken to without the other being disturbed.",
        "run" => "A run configuration is a named command line, a folder and some environment variables, kept in the project. Starting one runs the program in a pseudoterminal, so `run output` is what it would have printed to a terminal — which is how to start a dev server, read the port out of its log, use it, and stop it, with nobody watching.",
        "explorer" => "For requests to create, move, delete or list project paths, start here instead of using shell file operations. Quill updates its live tree immediately, and `new-file` also opens the file in a tab. `explorer files` leaves out `target`, `node_modules` and `__pycache__`.",
        "modal" => "One set of commands drives all of them: open it, type in it, read its results, choose a row, accept or cancel. A modal added to Quill later is driven with these same commands.",
        "settings" => "The names are the ones in Quill's own `settings.conf`, so there is one vocabulary rather than two. A change takes effect at once, in every tab, and is written to the file.",
        "theme" => "A theme says what every name in Quill's own palette means, which drawn icon set the rail and the explorer use, and — when it names all nine token colours — how code is coloured in every language at once. Quill's own theme names none of the nine, so each language plugin's scheme is what colours its files until a theme is chosen. `settings set appearance.theme` reaches the same code; these exist because a setting cannot say what themes there are.",
        "plugins" => "A plugin describes a language: its extensions, its keywords and a colour per kind of token. Nothing in one is executed and nothing is fetched over a network.",
        "git" => "Use this tool first when asked for git status, uncommitted work, changed files or a diff in the open project; do not begin by running git in a shell. Call `status` for the branch and exact staged, unstaged and untracked file list, then `action` with `name: show-diff` and `path` to open a changed file's diff in Quill. These still run the machine's real git with its credential helper, SSH agent, configuration and hooks, on a thread; `wait` holds the answer open.",
        "action" => "The escape hatch, and the guarantee: every entry on every menu has a name here, and the list is built by walking the real menus, so a menu entry added to Quill tomorrow can be run from the command line tomorrow.",
        "project" => "A project is a window. Opening a second project is `quill-cli launch <folder>`, which starts a second Quill; `project open` changes the folder this window is showing.",
        "mcp" => "The Model Context Protocol server, which is how an AI agent discovers and drives Quill without being handed a document first. Its tools are generated from this same catalogue, so a command added to Quill is a tool the day it is added.",
        _ => "",
    }
}

const fn argument(name: &'static str, required: bool, help: &'static str) -> Argument {
    Argument { name, required, rest: false, help }
}

/// An argument that takes the rest of the line, so the text after it needs no quoting.
const fn rest(name: &'static str, required: bool, help: &'static str) -> Argument {
    Argument { name, required, rest: true, help }
}

const fn switch(name: &'static str, help: &'static str) -> Flag {
    Flag { name, value: None, help }
}

const fn option(name: &'static str, value: &'static str, help: &'static str) -> Flag {
    Flag { name, value: Some(value), help }
}

const NO_ARGUMENTS: &[Argument] = &[];
const NO_FLAGS: &[Flag] = &[];

/// The one list.
pub const COMMANDS: &[Command] = &[
    // ---------------------------------------------------------------- the CLI and a whole Quill
    Command {
        area: "",
        verb: "status",
        summary: "Everything about the window in one answer: its version and build date, the project, the tabs, the panes, the terminal, the modal that is open, the settings and git. Ask for one part with --section and the answer is only that part.",
        arguments: NO_ARGUMENTS,
        flags: &[option("section", "name", "One part of the answer: editor, tabs, panes, panels, explorer, terminal, modal, settings, git, window, project or message. Several, comma-separated, for more than one. The whole answer when it is left out.")],
        examples: &["quill-cli status --json", "quill-cli status --section panes --json"],
        local: false,
    },
    Command {
        area: "",
        verb: "instances",
        summary: "The Quill windows that are running, with the port and the project of each. Answered without talking to any of them.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli instances --json"],
        local: true,
    },
    Command {
        area: "",
        verb: "launch",
        summary: "Start another Quill on a folder and wait until it answers.",
        arguments: &[argument("folder", false, "The project to open. The current folder when it is left out.")],
        flags: &[
            option("timeout", "milliseconds", "How long to wait for the new window to answer. 20000 by default."),
            switch("no-wait", "Return as soon as the process starts, without waiting for it to answer."),
        ],
        examples: &["quill-cli launch C:\\jason\\dev\\quill", "quill-cli launch . --timeout 40000"],
        local: true,
    },
    Command {
        area: "",
        verb: "quit",
        summary: "Close the window. Its settings and what it had open are written down first, as they are when it is closed by hand.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli quit"],
        local: false,
    },
    Command {
        area: "",
        verb: "commands",
        summary: "Every command this CLI has, as data: the areas, the arguments, the flags and the examples. This is what to read first when a program or an agent is driving Quill.",
        arguments: &[argument("name", false, "One command, such as `terminal send`, instead of all of them.")],
        flags: NO_FLAGS,
        examples: &["quill-cli commands --json", "quill-cli commands \"modal open\" --json"],
        local: true,
    },
    Command {
        area: "",
        verb: "version",
        summary: "What version this command line tool is. The version and build date of the Quill editor it is talking to are in `status`, and `modal open about` shows them in the window.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli version"],
        local: true,
    },
    // ------------------------------------------------------------------------------- the window
    Command {
        area: "window",
        verb: "screenshot",
        summary: "Write what the window is showing to a PNG file. The picture is of the real window, so it is how what a command did can be looked at.",
        arguments: &[argument("file", true, "Where to write the PNG. A folder that is not there is made.")],
        flags: &[option("timeout", "milliseconds", "How long to wait for the picture. 5000 by default.")],
        examples: &["quill-cli window screenshot _agent_output/after.png"],
        local: false,
    },
    Command {
        area: "window",
        verb: "focus",
        summary: "Bring the window to the front and give it the keyboard.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli window focus"],
        local: false,
    },
    Command {
        area: "window",
        verb: "size",
        summary: "Read how large the window is, or set it. A fixed size is what makes two screenshots comparable.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("width", "points", "How wide to make it."),
            option("height", "points", "How tall to make it."),
        ],
        examples: &["quill-cli window size", "quill-cli window size --width 1100 --height 720"],
        local: false,
    },
    Command {
        area: "window",
        verb: "position",
        summary: "Read where the window is on the screen, or move it.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("x", "points", "How far from the left of the screen."),
            option("y", "points", "How far from the top of the screen."),
        ],
        examples: &["quill-cli window position --x 40 --y 40"],
        local: false,
    },
    Command {
        area: "window",
        verb: "message",
        summary: "Read the line the status bar is showing, or put a line of your own there.",
        arguments: &[rest("text", false, "What to show. The line is cleared when this is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli window message", "quill-cli window message Ready for the next step"],
        local: false,
    },
    // ------------------------------------------------------------------------------ the browser
    Command {
        area: "browser",
        verb: "open",
        summary: "Render an HTTP address or local HTML file in a new Quill tab.",
        arguments: &[rest("address", true, "An HTTP or HTTPS address, or an HTML path relative to the project folder.")],
        flags: NO_FLAGS,
        examples: &["quill-cli browser open https://example.com", "quill-cli browser open examples/site/index.html"],
        local: false,
    },
    Command {
        area: "browser",
        verb: "status",
        summary: "Read the address, title, loading state, whether this is the tab the one view is pointed at, and the history directions of the rendered tab that is showing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli browser status --json"],
        local: false,
    },
    Command {
        area: "browser",
        verb: "back",
        summary: "Go back through the rendered tab's own history, which is kept apart from every other tab's.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli browser back"],
        local: false,
    },
    Command {
        area: "browser",
        verb: "forward",
        summary: "Go forward through the rendered tab's own history.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli browser forward"],
        local: false,
    },
    Command {
        area: "browser",
        verb: "reload",
        summary: "Reload the rendered tab that is showing, including its linked local resources.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli browser reload"],
        local: false,
    },
    // --------------------------------------------------------------------------------- the tabs
    Command {
        area: "tab",
        verb: "open",
        summary: "Open a file in a tab and show it. A picture opens as a picture; anything else opens as text.",
        arguments: &[argument("path", true, "The file. A relative path is resolved against the project folder.")],
        flags: &[switch("permanent", "Open it as a tab of its own rather than reusing the tab a single click reuses.")],
        examples: &["quill-cli tab open README.md", "quill-cli tab open design/style-guide.md --permanent"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "list",
        summary: "The tabs that are open, in order, with the path, the name and whether each has unsaved changes.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli tab list --json"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "show",
        summary: "Show a tab that is already open.",
        arguments: &[argument("tab", true, "Its number counting from 0, or its name, or its path.")],
        flags: NO_FLAGS,
        examples: &["quill-cli tab show 2", "quill-cli tab show README.md"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "close",
        summary: "Close a tab. A tab with unsaved changes is written first, which is what closing one by hand does. Closing the last one leaves an empty untitled tab rather than no tab at all.",
        arguments: &[argument("tab", false, "Its number, name or path. The tab that is showing when it is left out.")],
        flags: &[switch("discard", "Close it without writing what was typed into it.")],
        examples: &["quill-cli tab close", "quill-cli tab close notes.md", "quill-cli tab close --discard"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "next",
        summary: "Show the next tab, wrapping round at the end.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli tab next"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "previous",
        summary: "Show the previous tab.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli tab previous"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "move",
        summary: "Move a tab along its strip, or into another pane, which is what dragging it does. The position counts the tabs of the pane it is going to, as they are on the screen now.",
        arguments: &[argument("position", true, "Where it goes, counting from 0. Past the end means the end.")],
        flags: &[
            option("tab", "tab", "Which tab to move: its number, name or path. The tab that is showing when it is left out."),
            option("pane", "number", "Which pane to move it into, counting from 0. The pane it is already in when it is left out."),
        ],
        examples: &["quill-cli tab move 0", "quill-cli tab move 0 --tab notes.md --pane 1"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "save",
        summary: "Write the tab that is showing back to its file.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli tab save"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "save-as",
        summary: "Write the tab that is showing to another file, and go on editing that one.",
        arguments: &[argument("path", true, "Where to write it.")],
        flags: NO_FLAGS,
        examples: &["quill-cli tab save-as notes/copy.md"],
        local: false,
    },
    Command {
        area: "tab",
        verb: "reload",
        summary: "Read the file from disk again. A tab with unsaved changes is refused unless you say to throw them away, because there is no undo for that.",
        arguments: NO_ARGUMENTS,
        flags: &[switch("discard", "Reload even though the tab has unsaved changes, losing them.")],
        examples: &["quill-cli tab reload", "quill-cli tab reload --discard"],
        local: false,
    },
    // -------------------------------------------------------------------------------- the panes
    Command {
        area: "pane",
        verb: "list",
        summary: "The panes the editing area is split into, with the tabs in each, which tab is showing in each, and which pane has the keyboard.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli pane list --json"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "split",
        summary: "Put a pane to the right of the one with the keyboard and move the tab that is showing into it. A pane holding only that tab keeps it and the new pane opens empty.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli pane split"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "move",
        summary: "Move the tab that is showing into the pane beside it.",
        arguments: &[argument("direction", true, "left or right.")],
        flags: NO_FLAGS,
        examples: &["quill-cli pane move right", "quill-cli pane move left"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "focus",
        summary: "Put the keyboard in a pane, so that the next file opened lands in it.",
        arguments: &[argument("pane", true, "Its number counting from 0, left to right.")],
        flags: NO_FLAGS,
        examples: &["quill-cli pane focus 1"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "width",
        summary: "Set one pane's share of the editing area, which is what dragging the divider between two panes does. The other panes share what is left.",
        arguments: &[
            argument("pane", true, "Its number counting from 0."),
            argument("fraction", true, "Its share of the width, between 0.05 and 0.95."),
        ],
        flags: NO_FLAGS,
        examples: &["quill-cli pane width 0 0.35"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "unsplit",
        summary: "Fold the pane that has the keyboard into the one beside it, keeping its tabs.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli pane unsplit"],
        local: false,
    },
    Command {
        area: "pane",
        verb: "unsplit-all",
        summary: "Put every tab back into one pane.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli pane unsplit-all"],
        local: false,
    },
    // ------------------------------------------------------------------------------- the editor
    Command {
        area: "editor",
        verb: "status",
        summary: "What the tab that is showing holds: its path, how many lines, where the caret is, what is selected, whether it has unsaved changes and which view mode it is in.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor status --json"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "text",
        summary: "Read the text of the tab that is showing.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("from-line", "number", "The first line to read, counting from 1."),
            option("to-line", "number", "The last line to read, counting from 1."),
        ],
        examples: &["quill-cli editor text", "quill-cli editor text --from-line 1 --to-line 20"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "set-text",
        summary: "Replace everything in the tab that is showing. One undo puts it back.",
        arguments: &[rest("text", false, "The new text. Use --from-file instead for anything long.")],
        flags: &[option("from-file", "path", "Read the new text from this file rather than from the command line.")],
        examples: &["quill-cli editor set-text # Notes", "quill-cli editor set-text --from-file draft.md"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "insert",
        summary: "Type text at the caret, replacing the selection if there is one.",
        arguments: &[rest("text", true, "What to type. \\n is a new line and \\t is a tab.")],
        flags: NO_FLAGS,
        examples: &["quill-cli editor insert Hello", "quill-cli editor insert \"one\\ntwo\""],
        local: false,
    },
    Command {
        area: "editor",
        verb: "caret",
        summary: "Read where the caret is, or move it. Lines and columns count from 1, which is what the status bar shows.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("line", "number", "The line to move to."),
            option("column", "number", "The column to move to. The start of the line when it is left out."),
        ],
        examples: &["quill-cli editor caret", "quill-cli editor caret --line 42 --column 5"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "select",
        summary: "Select some of the text, all of it, or none of it.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("all", "Select the whole document."),
            switch("none", "Drop the selection, leaving the caret where it was."),
            option("from-line", "number", "The line the selection starts on."),
            option("from-column", "number", "The column it starts at. 1 when it is left out."),
            option("to-line", "number", "The line it ends on."),
            option("to-column", "number", "The column it ends at. The end of the line when it is left out."),
        ],
        examples: &["quill-cli editor select --all", "quill-cli editor select --from-line 3 --to-line 6"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "indent",
        summary: "Indent each line the selection touches, or the line the caret is on when nothing is selected, by one tab at the start of the line — or one space with --space. This is what Tab and Space do over a selection in the editing area, and the selection stays over the text it covered.",
        arguments: NO_ARGUMENTS,
        flags: &[switch("space", "Indent with a space rather than a tab, which is what the Space key does.")],
        examples: &["quill-cli editor indent", "quill-cli editor indent --space"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "dedent",
        summary: "Remove one indent from each line the selection touches, or the caret's line when nothing is selected — one tab, or one space with --space. This is what Shift+Tab and Shift+Space do over a selection. A line with none, or indented with the other unit, is left alone.",
        arguments: NO_ARGUMENTS,
        flags: &[switch("space", "Remove a space rather than a tab, which is what Shift+Space does.")],
        examples: &["quill-cli editor dedent", "quill-cli editor dedent --space"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "undo",
        summary: "Undo the last edit in the tab that is showing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor undo"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "redo",
        summary: "Redo the edit that was last undone.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor redo"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "view",
        summary: "Choose how a file with a preview is shown: the source, the source and the preview side by side, or the preview. Markdown and Mermaid files have one; nothing else does, and only a file with a preview can be shown any way but raw.",
        arguments: &[argument("mode", true, "raw, side or preview.")],
        flags: NO_FLAGS,
        examples: &["quill-cli editor view preview", "quill-cli editor view side"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "scroll",
        summary: "Read how far the tab that is showing is scrolled, or scroll it. With no flags it reports both halves of the side by side view. In side by side the other half follows, exactly as it does when you scroll with the wheel.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("line", "number", "Scroll so this line is at the top, counting from 1."),
            option("to", "points", "Scroll to this many points down the page."),
            switch("top", "Scroll to the top."),
            switch("bottom", "Scroll to the bottom."),
            switch("preview", "Scroll the Markdown preview rather than the source."),
        ],
        examples: &[
            "quill-cli editor scroll --json",
            "quill-cli editor scroll --line 120",
            "quill-cli editor scroll --preview --top",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "preview",
        summary: "Read the preview of the tab that is showing: a Markdown page as plain text with where its pictures and diagrams are, or, for a Mermaid file, what the diagram came out as.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor preview --json"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "preview-select",
        summary: "What is selected in the Markdown preview, and selecting something in it. The preview is read only, so a selection there is for reading and copying rather than editing; the offsets are into the preview's own text, which is what `editor preview` prints.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("from", "bytes", "Where the selection starts in the preview's text."),
            option("to", "bytes", "Where it ends. The end of the text when it is left out."),
            switch("all", "Select the whole preview."),
            switch("none", "Select nothing."),
            switch("copy", "Put whatever is selected on the clipboard."),
        ],
        examples: &[
            "quill-cli editor preview-select --json",
            "quill-cli editor preview-select --all --copy",
            "quill-cli editor preview-select --from 0 --to 40",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "definition",
        summary: "Where a name is defined, from Quill's live open tabs and project symbol index. Give the name directly or leave it out for the word at the caret; every candidate is printed best first and --open navigates through the editor.",
        arguments: &[argument("name", false, "The name to find. The word at the caret when it is left out.")],
        flags: &[
            option("offset", "bytes", "Ask about this position in the file rather than about the caret."),
            option("line", "number", "Ask about this line, counting from 1."),
            option("column", "number", "The column on that line. 1 when it is left out."),
            switch("open", "Go to the best candidate, opening its file as a tab."),
        ],
        examples: &[
            "quill-cli editor definition --json",
            "quill-cli editor definition Rect --open --json",
            "quill-cli editor definition --line 42 --column 9 --open",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "references",
        summary: "Use this instead of grep to find every place a name is used across the project: the file, line, column and whether it is code or a word inside a comment or string. Reads unsaved open tabs as they stand and everything else from the disk.",
        arguments: &[argument("name", false, "The name to look for. The word at the caret when it is left out.")],
        flags: &[
            option("timeout", "milliseconds", "How long to wait for the search. 10000 by default."),
            switch("code-only", "Leave out the matches inside comments and strings."),
        ],
        examples: &[
            "quill-cli editor references --json",
            "quill-cli editor references open_the_match --json",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "rename",
        summary: "Use this instead of file edits to rename a symbol everywhere through Quill's role-aware references. Comments and strings stay untouched unless included; without --apply it previews, and applying edits each open tab as one undo step before safely rewriting closed files.",
        arguments: &[argument("new-name", true, "What to call it. It has to be a word of this language and not one of its keywords.")],
        flags: &[
            option("name", "text", "Rename this name rather than the word at the caret."),
            option("scope", "file|project", "Which files to change. The default follows what the name resolves to: a variable or a name with no known definition is this file, and a function, type, constant or module is the project."),
            option("include", "comments,strings", "Also change the matches inside comments or strings, which are left alone by default."),
            option("timeout", "milliseconds", "How long to wait for the search that finds them. 10000 by default."),
            switch("apply", "Make the change. Without it the change set is printed and nothing is edited."),
        ],
        examples: &[
            "quill-cli editor rename open_the_result --json",
            "quill-cli editor rename open_the_result --apply",
            "quill-cli editor rename total --scope project --include comments --apply",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "complete",
        summary: "The names a word could become, best first, with what each row is and where it came from. By default the word is read from the document at the caret; --stem asks hypothetically without editing the document. Inside an import the rows are what can be imported instead. --choose applies a real document row exactly as Enter would.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("offset", "bytes", "Ask about this position in the file rather than about the caret."),
            option("line", "number", "Ask about this line, counting from 1."),
            option("column", "number", "The column on that line. 1 when it is left out."),
            option("stem", "text", "Ask what this hypothetical word would offer at the position, without inserting it or changing the document."),
            option("limit", "number", "Print at most this many rows. All of them when it is left out."),
            option("choose", "name", "Apply this row to the word being typed, as Enter would. It has to be one of the names offered."),
        ],
        examples: &[
            "quill-cli editor complete --json",
            "quill-cli editor complete --stem ar --limit 5 --json",
            "quill-cli editor complete --limit 5 --json",
            "quill-cli editor complete --choose draw_frame",
            "quill-cli editor complete --choose ./layout",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "navigate-back",
        summary: "Go back to where the caret was before the last jump, reopening the file if its tab was closed.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor navigate-back"],
        local: false,
    },
    Command {
        area: "editor",
        verb: "navigate-forward",
        summary: "Undo a navigate-back. Cleared by any new jump, exactly as a browser's forward button is.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor navigate-forward"],
        local: false,
    },
    // ------------------------------------------------------------------ the marked passages
    Command {
        area: "highlight",
        verb: "list",
        summary: "What is marked, in one file or across the whole project: where each passage is, what colour it is in, and the text under it.",
        arguments: &[argument("path", false, "The file to list. The tab that is showing when it is left out.")],
        flags: &[switch("all", "List every file in the project rather than one.")],
        examples: &["quill-cli highlight list --json", "quill-cli highlight list --all --json"],
        local: false,
    },
    Command {
        area: "highlight",
        verb: "add",
        summary: "Mark a passage in a colour. Give it lines and columns, or --text to mark every occurrence of some words. The file need not be open.",
        arguments: &[argument("path", false, "The file to mark. The tab that is showing when it is left out.")],
        flags: &[
            option("from-line", "number", "The line the passage starts on, counting from 1."),
            option("from-column", "number", "The column it starts at. 1 when it is left out."),
            option("to-line", "number", "The line it ends on. The line it started on when it is left out."),
            option("to-column", "number", "The column it ends at. The end of the line when it is left out."),
            option("text", "words", "Mark every occurrence of these words in the file instead of a range."),
            option("color", "name", "yellow, green, blue, pink, or a colour of your own as #rrggbb or #rrggbbaa. Yellow when it is left out."),
        ],
        examples: &[
            "quill-cli highlight add --from-line 12 --to-line 18",
            "quill-cli highlight add src/main.rs --from-line 40 --to-line 44 --color blue",
            "quill-cli highlight add src/main.rs --text \"unwrap()\" --color pink",
        ],
        local: false,
    },
    Command {
        area: "highlight",
        verb: "clear",
        summary: "Take marks away: a range of lines, a whole file, or every file in the project.",
        arguments: &[argument("path", false, "The file to clear. The tab that is showing when it is left out.")],
        flags: &[
            option("from-line", "number", "The first line to clear, counting from 1. The whole file when it is left out."),
            option("to-line", "number", "The last line to clear. The line it started on when it is left out."),
            switch("all", "Clear every file in the project."),
        ],
        examples: &[
            "quill-cli highlight clear",
            "quill-cli highlight clear src/main.rs --from-line 40 --to-line 44",
            "quill-cli highlight clear --all",
        ],
        local: false,
    },
    Command {
        area: "highlight",
        verb: "apply",
        summary: "Mark many passages across many files in one go, from a JSON array of {path, fromLine, toLine, fromColumn, toColumn, color} objects.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("from-file", "path", "Read the JSON array from this file."),
            option("json-text", "json", "The JSON array itself, for a short list. Quote it."),
            switch("replace", "Clear every mark in the project first, so what is applied is all there is."),
        ],
        examples: &[
            "quill-cli highlight apply --from-file marks.json",
            "quill-cli highlight apply --json-text '[{\"path\":\"src/main.rs\",\"fromLine\":1,\"toLine\":3}]'",
        ],
        local: false,
    },
    // ------------------------------------------------------------------ the collapsed blocks
    Command {
        area: "fold",
        verb: "list",
        summary: "Every block in the tab that is showing that can be collapsed: which line it starts on, which line it ends on, how many lines it hides, what kind of block it is, and whether it is collapsed now.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli fold list --json"],
        local: false,
    },
    Command {
        area: "fold",
        verb: "toggle",
        summary: "Collapse a block that is showing, or expand one that is collapsed. The block at the caret when no line is given. The answer is how many blocks are collapsed; --regions adds the list.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("line", "number", "The line the block starts on, counting from 1. `fold list` says which lines those are."),
            switch("regions", "Also answer with the list of every block and whether it is collapsed."),
        ],
        examples: &["quill-cli fold toggle", "quill-cli fold toggle --line 42", "quill-cli fold toggle --regions --json"],
        local: false,
    },
    Command {
        area: "fold",
        verb: "collapse",
        summary: "Collapse one block, every block in the file, or one block and every block inside it. The answer is how many blocks are collapsed; --regions adds the list.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("line", "number", "The line the block starts on, counting from 1."),
            switch("all", "Collapse every block in the file."),
            switch("recursive", "With --line, collapse that block and every block inside it rather than just that block."),
            switch("regions", "Also answer with the list of every block and whether it is collapsed."),
        ],
        examples: &["quill-cli fold collapse --all", "quill-cli fold collapse --line 42", "quill-cli fold collapse --line 42 --recursive", "quill-cli fold collapse --all --regions --json"],
        local: false,
    },
    Command {
        area: "fold",
        verb: "expand",
        summary: "Expand one block, show all again, or expand one block and every block inside it. The answer is how many blocks are collapsed; --regions adds the list.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("line", "number", "The line the block starts on, counting from 1."),
            switch("all", "Expand every block in the file."),
            switch("recursive", "With --line, expand that block and every block inside it, opening the whole of it rather than one level."),
            switch("regions", "Also answer with the list of every block and whether it is collapsed."),
        ],
        examples: &["quill-cli fold expand --all", "quill-cli fold expand --line 42", "quill-cli fold expand --line 42 --recursive", "quill-cli fold expand --all --regions --json"],
        local: false,
    },
    Command {
        area: "fold",
        verb: "others",
        summary: "Collapse everything that does not hold a marked passage, so only the marked parts of the file are left showing. Falls back to the selection when nothing is marked. The answer is how many blocks are collapsed; --regions adds the list.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("selection", "Keep what is selected rather than what is marked, even when there are marks."),
            switch("regions", "Also answer with the list of every block and whether it is collapsed."),
        ],
        examples: &["quill-cli fold others", "quill-cli fold others --selection", "quill-cli fold others --regions --json"],
        local: false,
    },
    // -------------------------------------------------------------------------------- the panels
    Command {
        area: "panel",
        verb: "list",
        summary: "Every panel Quill has — the explorer, the terminal, the run tile and the debug tile — which edge of the window each is docked to, where in that edge, how big it is, whether it is showing, and the rectangle it occupies on screen.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli panel list --json"],
        local: false,
    },
    Command {
        area: "panel",
        verb: "dock",
        summary: "Move a panel to an edge of the window: the same change dragging its header makes. A side can hold more than one panel, side by side, so the terminal can sit beside the explorer down the left.",
        arguments: &[
            argument("panel", true, "explorer, terminal, run or debug."),
            argument("side", true, "left, right, top or bottom."),
        ],
        flags: &[option(
            "position",
            "number",
            "Where in that side, counting the panels already there from the left, starting at 0. The end of the side when it is not given.",
        )],
        examples: &[
            "quill-cli panel dock terminal right",
            "quill-cli panel dock terminal left --position 0",
        ],
        local: false,
    },
    Command {
        area: "panel",
        verb: "size",
        summary: "Set how wide or how tall a panel is. A panel at the left or the right is read by its width and one along the top or the bottom by its height, so both are kept and moving a panel does not lose the size it had on the other side.",
        arguments: &[argument("panel", true, "explorer, terminal, run or debug.")],
        flags: &[
            option("width", "points", "How wide it is when it is a column at the left or the right."),
            option("height", "points", "How tall it is when it is in a strip along the top or the bottom."),
        ],
        examples: &["quill-cli panel size debug --width 640", "quill-cli panel size terminal --height 320"],
        local: false,
    },
    Command {
        area: "panel",
        verb: "zoom",
        summary: "Make everything in a panel bigger or smaller, which is what Ctrl/Cmd and the wheel over it does. The explorer and a pane a plugin contributed carry a multiplier of their own; the terminal, run and debug tiles are character grids and their zoom is the terminal's font size, so a zoom there walks `settings set terminal.font.size` and both say the same number.",
        arguments: &[
            argument("panel", true, "explorer, terminal, run, debug, or a contributed pane's <plugin>/<pane>."),
            argument("factor", false, "How much bigger than usual, between 0.5 and 3. Left out, it says what the panel is at now; `reset` puts it back to 1."),
        ],
        flags: NO_FLAGS,
        examples: &[
            "quill-cli panel zoom explorer 1.35",
            "quill-cli panel zoom agent-chat/chat",
            "quill-cli panel zoom explorer reset",
        ],
        local: false,
    },
    Command {
        area: "panel",
        verb: "reset",
        summary: "Put every panel back where a new Quill has it: the explorer down the left, the three tiles along the bottom, each at its starting size.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli panel reset"],
        local: false,
    },
    // ----------------------------------------------------------------------------- the terminal
    Command {
        area: "terminal",
        verb: "show",
        summary: "Show the terminal along the bottom, opening a shell in the project folder if there is not one already.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli terminal show"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "hide",
        summary: "Put the terminal away. The shells keep running.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli terminal hide"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "toggle",
        summary: "Show the terminal if it is hidden, and hide it if it is showing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli terminal toggle"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "new",
        summary: "Start another shell in a tab of its own, and show it.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli terminal new"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "list",
        summary: "The terminal tabs, with the name of each and which one is showing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli terminal list --json"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "select",
        summary: "Show one of the terminal tabs. The only verb that changes which tab is showing.",
        arguments: &[argument("index", false, "Its number, counting from 0. The --tab flag when it is given.")],
        flags: &[option("tab", "index", "Which tab to show, counting from 0.")],
        examples: &["quill-cli terminal select 1", "quill-cli terminal select --tab 1"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "close",
        summary: "Close a terminal tab. Closing the last one puts the terminal away.",
        arguments: &[argument("index", false, "Its number. The --tab flag when it is given, the tab that is showing when both are left out.")],
        flags: &[option("tab", "index", "Which tab to close, counting from 0. The one that is showing when it is left out.")],
        examples: &["quill-cli terminal close", "quill-cli terminal close --tab 1"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "rename",
        summary: "Call a terminal tab something else. The name stays put when the program in the tab sets a title of its own; an empty name puts the tab back to being named after its program.",
        arguments: &[rest("name", true, "What to call it. Everything after the verb is taken as the name, so it needs no quotes.")],
        flags: &[option("tab", "index", "Which tab, counting from 0. The one that is showing when it is left out.")],
        examples: &[
            "quill-cli terminal rename build",
            "quill-cli terminal rename --tab 1 the long running one",
        ],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "move",
        summary: "Move a terminal tab along the strip, which is what dragging one does.",
        arguments: &[argument("position", true, "Where it goes, counting the tabs as they are on the screen now from 0.")],
        flags: &[option("tab", "index", "Which tab to move, counting from 0. The one that is showing when it is left out.")],
        examples: &["quill-cli terminal move 0", "quill-cli terminal move --tab 2 0"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "send",
        summary: "Send a command to the shell in a terminal tab, the one that is showing when --tab is left out. Naming a tab does not show it. Enter is pressed for you unless you say not to.",
        arguments: &[rest("text", false, "The command. Everything after the verb is taken as the command, so it needs no quotes.")],
        flags: &[
            option("tab", "index", "Which tab to send to, counting from 0. The one that is showing when it is left out."),
            switch("no-enter", "Type the text and leave it on the prompt without running it."),
            option("key", "name", "Send a key instead of text: enter, tab, escape, up, down, left, right, backspace, ctrl-c, ctrl-d, ctrl-l."),
        ],
        examples: &[
            "quill-cli terminal send git status",
            "quill-cli terminal send --tab 1 cargo check",
            "quill-cli terminal send --key ctrl-c",
            "quill-cli terminal send --no-enter cd ..",
        ],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "read",
        summary: "Read what a terminal tab has on its screen, the one that is showing when --tab is left out. Reading a tab does not show it.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("tab", "index", "Which tab to read, counting from 0. The one that is showing when it is left out."),
            option("lines", "number", "Only the last so many lines."),
            option("wait-for", "text", "Wait until this text is on the named tab's screen before answering, which is how to wait for a command to finish."),
            option("timeout", "milliseconds", "How long to wait for --wait-for. 10000 by default."),
        ],
        examples: &[
            "quill-cli terminal read --lines 20",
            "quill-cli terminal read --tab 1",
            "quill-cli terminal read --tab 1 --wait-for \"$\" --timeout 15000",
        ],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "height",
        summary: "Read how tall the terminal tile is, or set it. The same measurement dragging its top edge changes.",
        arguments: &[argument("points", false, "How tall to make it. Read it when this is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli terminal height 400"],
        local: false,
    },
    // ---------------------------------------------------------------------- the run configurations
    Command {
        area: "run",
        verb: "list",
        summary: "The project's run configurations: the name, the command, the folder and the environment of each, whether it is permanent, temporary or a suggestion, and what its run is doing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli run list --json"],
        local: false,
    },
    Command {
        area: "run",
        verb: "add",
        summary: "Keep a new run configuration in the project. The command is one line: the first word is the program and the rest are its arguments, and no shell runs it, so nothing is expanded and && is an argument. It says so when the program cannot be found on this window's PATH, and keeps the configuration anyway.",
        arguments: &[
            argument("name", true, "What to call it, which is what the widget and the Run menu show."),
            rest("command", true, "The command line. Everything after the name is taken as the command, so it needs no quotes."),
        ],
        flags: &[
            option("directory", "path", "The folder it runs in, relative to the project. The project itself when it is left out."),
            option("env", "pairs", "NAME=value pairs separated by semicolons."),
        ],
        examples: &[
            "quill-cli run add \"Dev server\" node server.js --port 3000",
            "quill-cli run add build cargo build --release --directory crates/quill-app",
            "quill-cli run add serve npm run dev --env \"PORT=3000; DEBUG=app:*\"",
        ],
        local: false,
    },
    Command {
        area: "run",
        verb: "remove",
        summary: "Take a run configuration away. One whose program is still running is stopped first.",
        arguments: &[argument("name", true, "The configuration, as `run list` gives it.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run remove \"Dev server\""],
        local: false,
    },
    Command {
        area: "run",
        verb: "start",
        summary: "Run a configuration, showing the run tile. Starting one that is already running stops it and starts it again rather than making a second copy. A detector's suggestion started this way is kept as a temporary configuration. A program that could not be started is a failure carrying the reason, not a reply that says nothing ran.",
        arguments: &[argument("name", false, "The configuration. The chosen one when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run start", "quill-cli run start \"Dev server\""],
        local: false,
    },
    Command {
        area: "run",
        verb: "stop",
        summary: "Stop a run: the interrupt a program can catch, and a hard kill two seconds later or on a second stop. The tab stays, holding what the program wrote. Stopping when nothing is running is a failure that says so rather than a quiet success.",
        arguments: &[argument("name", false, "The configuration. The chosen one when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run stop", "quill-cli run stop \"Dev server\""],
        local: false,
    },
    Command {
        area: "run",
        verb: "rerun",
        summary: "Stop a run and start it again, whatever state it was in.",
        arguments: &[argument("name", false, "The configuration. The chosen one when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run rerun"],
        local: false,
    },
    Command {
        area: "run",
        verb: "select",
        summary: "Choose which configuration the widget's play button, the Run menu and `run start` with no name all mean.",
        arguments: &[argument("name", true, "The configuration.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run select \"Dev server\""],
        local: false,
    },
    Command {
        area: "run",
        verb: "output",
        summary: "What a run has written, as text. It ran in a pseudoterminal, so this is what it would have printed to a terminal — colours and progress bars included, with the escape sequences already read.",
        arguments: &[argument("name", false, "The configuration. The run that is showing when it is left out.")],
        flags: &[
            option("tail", "number", "Only the last so many lines."),
            option("wait-for", "text", "Wait until this text has been written before answering, which is how to wait for a server to say it is listening."),
            option("timeout", "milliseconds", "How long to wait for --wait-for. 10000 by default."),
        ],
        examples: &[
            "quill-cli run output --tail 20",
            "quill-cli run output \"Dev server\" --wait-for \"Listening on\" --timeout 30000",
        ],
        local: false,
    },
    Command {
        area: "run",
        verb: "status",
        summary: "Whether a run is going, and what it ended with: running, finished, stopped, or the exit code it chose.",
        arguments: &[argument("name", false, "The configuration. The chosen one when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli run status --json", "quill-cli run status \"Dev server\" --json"],
        local: false,
    },
    // ---------------------------------------------------------------------------- the debugger
    Command {
        area: "debug",
        verb: "start",
        summary: "Run a configuration under its debugger, showing the debug tile. The file that is open decides which debugger: its language names one, or the session refuses with a sentence saying what to install. Starting one replaces the session that was running.",
        arguments: &[argument("name", false, "The configuration. The chosen one when it is left out.")],
        flags: &[
            switch("wait-for-pause", "Wait until the program stops somewhere before answering, so a script can set a breakpoint, start, and read a variable in three commands."),
            option("timeout", "milliseconds", "How long to wait for --wait-for-pause. 30000 by default."),
        ],
        examples: &[
            "quill-cli debug start",
            "quill-cli debug start \"Dev server\" --wait-for-pause",
        ],
        local: false,
    },
    Command {
        area: "debug",
        verb: "stop",
        summary: "End the session: the polite request first, and a hard disconnect on a second stop or two seconds later. The debuggee's tab in the run tile stays, holding what it wrote.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli debug stop"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "continue",
        summary: "Let the program run on to the next breakpoint.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("wait-for-pause", "Wait until it stops again before answering."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug continue --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "step-over",
        summary: "Run the current line and stop on the next one, without going into any call it makes.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("wait-for-pause", "Wait until it stops again before answering."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug step-over --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "step-into",
        summary: "Go into the call on the current line and stop at its first line.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("wait-for-pause", "Wait until it stops again before answering."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug step-into --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "step-out",
        summary: "Finish the function the program is in and stop in whatever called it.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("wait-for-pause", "Wait until it stops again before answering."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug step-out --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "run-to",
        summary: "Run until the program reaches a line, then stop there. A temporary breakpoint, a resume, and the breakpoint taken away again — which is how every debugger builds this.",
        arguments: &[
            argument("path", true, "The file, relative to the project or absolute."),
            argument("line", true, "The line, counting from 1."),
        ],
        flags: &[
            switch("wait-for-pause", "Wait until it stops before answering."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug run-to src/main.rs 42 --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "breakpoint",
        summary: "Where the program is to stop. `add` and `remove` take a file and a line; `list` prints every one in the project, with what the debugger said about it while a session is running. Breakpoints are kept in .quill/breakpoints.conf and move with the text as the file is edited.",
        arguments: &[
            argument("action", true, "add, remove, enable, disable, list, or clear."),
            argument("path", false, "The file. Not needed for list or clear."),
            argument("line", false, "The line, counting from 1."),
        ],
        flags: &[
            option("condition", "expression", "Stop only while this is true. The debugger evaluates it, in the program's own language."),
            option("log", "message", "Print this instead of stopping, which is what other editors call a logpoint. The debugger formats it, so {name} reads a variable."),
        ],
        examples: &[
            "quill-cli debug breakpoint add src/main.rs 42",
            "quill-cli debug breakpoint add src/main.rs 42 --condition \"attempts > 3\"",
            "quill-cli debug breakpoint list --json",
            "quill-cli debug breakpoint remove src/main.rs 42",
        ],
        local: false,
    },
    Command {
        area: "debug",
        verb: "frames",
        summary: "The call stack of the stopped thread, one frame a line: the function, the file and the line. Adapter-marked runtime frames are hidden unless --include-subtle asks for the complete stack. Answered from what the debugger has already been asked, so it costs nothing.",
        arguments: NO_ARGUMENTS,
        flags: &[switch("include-subtle", "Include adapter-marked runtime frames, which are normally hidden so the stack leads with application code.")],
        examples: &["quill-cli debug frames --json", "quill-cli debug frames --include-subtle"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "variables",
        summary: "The variables of the frame that is showing. Only what has been read is printed, because a debugger reads a structure's contents only when somebody opens it; --expand asks for one row's children by name.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("frame", "number", "Which frame, counting from 0 at the top. The one that is showing when it is left out."),
            option("expand", "path", "Read the children of this row and print them, naming it the way `variables` prints it — Locals/items."),
        ],
        examples: &[
            "quill-cli debug variables --json",
            "quill-cli debug variables --expand Locals/items",
        ],
        local: false,
    },
    Command {
        area: "debug",
        verb: "set-value",
        summary: "Change a variable in the running program. The answer is the value as the debugger now sees it, which is not always what was typed.",
        arguments: &[
            argument("path", true, "The row, as `variables` names it — Locals/count."),
            rest("value", true, "The new value, in the program's own language."),
        ],
        flags: NO_FLAGS,
        examples: &["quill-cli debug set-value Locals/count 7"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "hover",
        summary: "What a person sees when they rest the pointer on a name while the program is stopped: the expression Quill reads at that position, its value and type, and its children as a tree. Reads the name plus the field path in front of it, so a point on `count` in `self.items.count` asks about the whole of it. Unlike `evaluate`, the answer can be walked into with --expand.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("offset", "bytes", "Ask about this position in the file rather than about the caret."),
            option("line", "number", "Ask about this line, counting from 1."),
            option("column", "number", "The column on that line. 1 when it is left out."),
            option("expression", "text", "Ask about this expression outright rather than about a position, which is how a value from `evaluate` is expanded."),
            option("expand", "path", "Open this row and read its children, naming it the way the rows are printed - self.items/0."),
            option("timeout", "milliseconds", "How long to wait for the debugger. 10000 by default."),
        ],
        examples: &[
            "quill-cli debug hover --line 42 --column 9 --json",
            "quill-cli debug hover --expression self.items --expand self.items/0",
        ],
        local: false,
    },
    Command {
        area: "debug",
        verb: "set-expression",
        summary: "Assign to whatever an expression names in the running program. The other half of `set-value`: that one names a row that has already been read, and this one names the target in the program's own language, so it reaches a value nothing has opened yet. A debugger that cannot compile an assignment still changes a plain variable it has already shown, and says so plainly when it can do neither.",
        arguments: &[
            argument("expression", true, "What to assign to, in the program's own language - self.items.count."),
            rest("value", true, "The new value, in the program's own language."),
        ],
        flags: NO_FLAGS,
        examples: &["quill-cli debug set-expression self.items.count 7"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "evaluate",
        summary: "Evaluate an expression in the frame that is showing. The debugger's own answer, or its own refusal.",
        arguments: &[rest("expression", true, "The expression. Everything after the verb is taken as it was typed, so it needs no quotes.")],
        flags: &[option("timeout", "milliseconds", "How long to wait for the answer. 10000 by default.")],
        examples: &["quill-cli debug evaluate items.len()"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "watch",
        summary: "Expressions re-evaluated at every stop. `add` and `remove` take one; `list` prints them with their last answers.",
        arguments: &[
            argument("action", true, "add, remove, or list."),
            rest("expression", false, "The expression, for add and remove."),
        ],
        flags: NO_FLAGS,
        examples: &["quill-cli debug watch add attempts", "quill-cli debug watch list --json"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "output",
        summary: "What the debugger itself has said: what it loaded, what it could not find, and why it refused something. Not the program's own output, which goes to the run tile and is read with `run output`.",
        arguments: NO_ARGUMENTS,
        flags: &[option("tail", "number", "Only the last so many lines.")],
        examples: &["quill-cli debug output --tail 20"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "status",
        summary: "Whether a session is running and what it is doing: starting, running, paused with the file and line it stopped at, or ended with the code the program chose.",
        arguments: NO_ARGUMENTS,
        flags: &[
            switch("wait-for-pause", "Wait until the program stops before answering, which is how a script waits for a breakpoint it has already set."),
            option("timeout", "milliseconds", "How long to wait. 30000 by default."),
        ],
        examples: &["quill-cli debug status --json", "quill-cli debug status --wait-for-pause"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "adapters",
        summary: "Which debuggers this Quill drives, where each one is on this machine, what is missing, and the command that installs it. The first thing to run when a debug session will not start: it answers in fields under --json, so nothing has to be guessed at from a refusal.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli debug adapters", "quill-cli debug adapters --json"],
        local: false,
    },
    Command {
        area: "debug",
        verb: "install",
        summary: "Install a debug adapter by running its own install command in the run tile, where it can be watched with `run output` and stopped. Quill itself downloads nothing: what runs is a package manager, or an editor's extension installer, named by `debug adapters`.",
        arguments: &[argument("adapter", true, "Which debugger: lldb or node.")],
        flags: NO_FLAGS,
        examples: &["quill-cli debug install lldb"],
        local: false,
    },
    // ----------------------------------------------------------------------------- the explorer
    Command {
        area: "explorer",
        verb: "show",
        summary: "Show the file explorer down the left.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli explorer show"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "hide",
        summary: "Collapse the file explorer, leaving the rail of buttons.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli explorer hide"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "toggle",
        summary: "Show the explorer if it is hidden, and hide it if it is showing.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli explorer toggle"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "width",
        summary: "Read how wide the explorer is, or set it. The same measurement dragging its edge changes.",
        arguments: &[argument("points", false, "How wide to make it, from 150 to 620. Read it when this is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer width 320"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "filter",
        summary: "Read the explorer's filter box, or type into it. The tree then shows only what matches.",
        arguments: &[rest("text", false, "What to filter by. The box is cleared when this is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer filter tdd", "quill-cli explorer filter"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "expand",
        summary: "Open a folder in the tree, and every folder above it.",
        arguments: &[argument("path", true, "The folder, relative to the project or absolute.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer expand crates/quill-app/src"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "collapse",
        summary: "Shut a folder in the tree.",
        arguments: &[argument("path", false, "The folder. Every open folder is shut when this is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer collapse crates", "quill-cli explorer collapse"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "tree",
        summary: "The rows the explorer is showing, in order, with the depth of each and whether it is a folder.",
        arguments: NO_ARGUMENTS,
        flags: &[option("limit", "number", "At most this many rows. 200 by default.")],
        examples: &["quill-cli explorer tree --json"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "files",
        summary: "Every file in the project that Quill searches, which leaves out what a build wrote: target, node_modules and __pycache__.",
        arguments: NO_ARGUMENTS,
        flags: &[option("limit", "number", "At most this many paths. 500 by default.")],
        examples: &["quill-cli explorer files --limit 20 --json"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "select-open-file",
        summary: "Scroll the explorer to the file that is showing and select it, opening out the folders above it. It happens on its own when the tab changes; this asks for it by hand.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli explorer select-open-file"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "select",
        summary: "Set the row the explorer's own cursor is on, which is what Delete is about, or read it when no path is given. It is not the same as the tab that is showing.",
        arguments: &[argument("path", false, "The file or folder to select.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer select README.md", "quill-cli explorer select --json"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "delete",
        summary: "Delete a file or a folder. On Windows it goes to the Recycle Bin; everywhere else it is gone. No question is asked, because typing the command is the deliberate act the question exists to ask for.",
        arguments: &[argument("path", true, "The file or folder to delete.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer delete notes/old.md"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "move",
        summary: "Move a file or a folder into another folder, rewriting every import, use line and mod declaration in the project that names it. The same thing dragging a row in the explorer does.",
        arguments: &[
            argument("path", true, "The file or folder to move."),
            argument("folder", true, "The folder it goes into."),
        ],
        flags: &[
            switch("dry-run", "Print the whole change set and change nothing at all."),
            switch("no-refactor", "Move the bytes and leave every reference to them alone."),
        ],
        examples: &[
            "quill-cli explorer move src/app/layout.ts src/draw",
            "quill-cli explorer move src/app/layout.ts src/draw --dry-run --json",
        ],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "new-file",
        summary: "Make an empty file, create its parent folders, update Quill's live tree and open it in a tab. The same thing New -> File on the explorer's right click menu does, without the dialog.",
        arguments: &[argument("path", true, "Where the file goes, relative to the project or absolute.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer new-file notes/today.md"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "new-folder",
        summary: "Make a folder and every folder above it, updating Quill's live tree immediately. The same thing New -> Folder on the explorer's right click menu does, without the dialog.",
        arguments: &[argument("path", true, "Where the folder goes, relative to the project or absolute.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer new-folder src/services"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "reload",
        summary: "Read the project's folders again, so anything another program has just made appears. It happens on its own within a second; this asks for it now.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli explorer reload"],
        local: false,
    },
    Command {
        area: "explorer",
        verb: "reveal",
        summary: "Show a path in the platform's own file manager: Explorer on Windows, Finder on macOS.",
        arguments: &[argument("path", true, "The file or folder.")],
        flags: NO_FLAGS,
        examples: &["quill-cli explorer reveal README.md"],
        local: false,
    },
    // ------------------------------------------------------------------------------- the modals
    Command {
        area: "modal",
        verb: "list",
        summary: "The modals that can be opened, and which one is open now.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli modal list --json"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "open",
        summary: "Open a modal, and put something in its box in the same breath.",
        arguments: &[argument("name", true, "go-to-file, find-in-files, settings, about, new-file or rename.")],
        flags: &[
            option("query", "text", "Type this into the modal's box as it opens."),
            option("path", "path", "The folder a new file goes in, or the file being renamed. Needed by new-file and rename."),
            option("page", "name", "Which page the Settings modal shows: appearance, editor, plugins, terminal or mcp."),
        ],
        examples: &[
            "quill-cli modal open go-to-file --query mdrs",
            "quill-cli modal open find-in-files --query \"fn main\"",
            "quill-cli modal open settings --page terminal",
            "quill-cli modal open about",
            "quill-cli modal open new-file --path notes",
        ],
        local: false,
    },
    Command {
        area: "modal",
        verb: "state",
        summary: "What the modal that is open is showing: its name, what is in its box, how many results it has and which one is chosen.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli modal state --json"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "type",
        summary: "Put text in the box of the modal that is open, as though it had been typed.",
        arguments: &[rest("text", false, "What to put in the box. The box is cleared when this is left out.")],
        flags: &[switch("match-case", "Turn on Find in Files' match case tick box while typing.")],
        examples: &["quill-cli modal type quill-cli", "quill-cli modal type --match-case Quill"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "results",
        summary: "What the modal that is open has found: the files Go to File matched, or the lines Find in Files matched.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("limit", "number", "At most this many. 50 by default."),
            option("wait", "milliseconds", "Wait up to this long for a search that is still running to finish."),
        ],
        examples: &["quill-cli modal results --limit 10 --json", "quill-cli modal results --wait 5000 --json"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "choose",
        summary: "Move the chosen row in the modal that is open, without opening anything.",
        arguments: &[argument("index", true, "The row, counting from 0.")],
        flags: NO_FLAGS,
        examples: &["quill-cli modal choose 2"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "accept",
        summary: "Do what pressing Enter in the modal does: open the chosen file, jump to the chosen match, or press the modal's main button.",
        arguments: &[argument("index", false, "Choose this row first.")],
        flags: NO_FLAGS,
        examples: &["quill-cli modal accept", "quill-cli modal accept 0"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "cancel",
        summary: "Shut the modal that is open without doing anything, the way Escape does.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli modal cancel"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "move",
        summary: "Drag the modal that is open to a place on the window, the way its header does.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("x", "points", "How far from the left of the window its left edge goes."),
            option("y", "points", "How far from the top of the window its top edge goes."),
        ],
        examples: &["quill-cli modal move --x 60 --y 60"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "size",
        summary: "Resize the modal that is open, the way its edges do.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("width", "points", "How wide to make it."),
            option("height", "points", "How tall to make it."),
        ],
        examples: &["quill-cli modal size --width 900 --height 600"],
        local: false,
    },
    Command {
        area: "modal",
        verb: "reset",
        summary: "Put the modal that is open back in the middle at the size it asked for, the way a double click on its header does.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli modal reset"],
        local: false,
    },
    // ----------------------------------------------------------------------------- the settings
    Command {
        area: "settings",
        verb: "list",
        summary: "Every setting, with its value, what it means and what it will accept. The names are the ones in Quill's own settings file.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli settings list --json"],
        local: false,
    },
    Command {
        area: "settings",
        verb: "get",
        summary: "Read one setting.",
        arguments: &[argument("key", true, "The name, such as appearance.font.size.")],
        flags: NO_FLAGS,
        examples: &["quill-cli settings get appearance.font.size"],
        local: false,
    },
    Command {
        area: "settings",
        verb: "set",
        summary: "Change one setting. It takes effect at once, in every tab, and is written to the settings file.",
        arguments: &[
            argument("key", true, "The name, such as appearance.background.opacity."),
            rest("value", true, "The new value."),
        ],
        flags: NO_FLAGS,
        examples: &[
            "quill-cli settings set appearance.font.size 20",
            "quill-cli settings set appearance.background.opacity 0.5",
            "quill-cli settings set editor.line_numbers false",
            "quill-cli settings set terminal.shell cmd.exe",
            "quill-cli settings set appearance.font.family \"Courier New\"",
        ],
        local: false,
    },
    Command {
        area: "settings",
        verb: "reset",
        summary: "Put a setting, or every setting, back to what a Quill that has never been run has.",
        arguments: &[argument("key", false, "The setting. All of them when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli settings reset appearance.font.size", "quill-cli settings reset"],
        local: false,
    },
    Command {
        area: "settings",
        verb: "fonts",
        summary: "The font families this machine has that the editor can be set to.",
        arguments: NO_ARGUMENTS,
        flags: &[option("limit", "number", "At most this many. 100 by default.")],
        examples: &["quill-cli settings fonts --json"],
        local: false,
    },
    // ----------------------------------------------------------------------------------- themes
    Command {
        area: "theme",
        verb: "list",
        summary: "Every theme that can be chosen, with the plugin it came from and the six colours it is most recognisable by, and which one the window is painted in now.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli theme list --json"],
        local: false,
    },
    Command {
        area: "theme",
        verb: "show",
        summary: "One theme in full: every colour in Quill's palette by name, the nine token colours, and which drawn icon set it uses. The one the window is painted in when no theme is named.",
        arguments: &[argument("theme", false, "Its key, `themes-bundle-1/dracula`, or its name, `Monokai Pro`. The active one when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli theme show --json", "quill-cli theme show \"Monokai Pro\" --json"],
        local: false,
    },
    Command {
        area: "theme",
        verb: "set",
        summary: "Paint the window in a theme. It takes effect at once, in every tab and every pane, and is written to the settings file.",
        arguments: &[rest("theme", true, "Its key, `themes-bundle-1/dracula`, or its name, `Material Deep Ocean`.")],
        flags: &[
            option("accent", "colour", "One colour for everything the accent means, as #RRGGBB. `none` puts it back to the theme's own."),
            option("icons", "set", "Which drawn icon set to use: material, classic, or `follow` for whichever the theme names."),
        ],
        examples: &[
            "quill-cli theme set themes-bundle-1/dracula",
            "quill-cli theme set \"Monokai Pro\" --icons material",
            "quill-cli theme set quill/dark --accent none",
        ],
        local: false,
    },
    // ---------------------------------------------------------------------------------- plugins
    Command {
        area: "plugins",
        verb: "list",
        summary: "The language plugins Quill has, which of them are switched on, and what each one claims. They ship with Quill; nothing is fetched.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli plugins list --json"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "install",
        summary: "Write a plugin out into the settings folder, so its files can be read and changed.",
        arguments: &[argument("id", true, "The plugin's id, as `plugins list` gives it.")],
        flags: NO_FLAGS,
        examples: &["quill-cli plugins install rust"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "enable",
        summary: "Switch a plugin on, so it colours the files it claims.",
        arguments: &[argument("id", true, "The plugin's id.")],
        flags: NO_FLAGS,
        examples: &["quill-cli plugins enable rust"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "disable",
        summary: "Switch a plugin off. Its files stay where they are.",
        arguments: &[argument("id", true, "The plugin's id.")],
        flags: NO_FLAGS,
        examples: &["quill-cli plugins disable rust"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "show",
        summary: "One plugin in full: its manifest, whether it describes a language or draws, what it contributes, and every command it answers.",
        arguments: &[argument("id", true, "The plugin's id, as `plugins list` gives it.")],
        flags: NO_FLAGS,
        examples: &["quill-cli plugins show agent-tasks --json"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "reload",
        summary: "Read every plugin manifest from disk again, so one changed by hand takes effect with no restart. One that will not parse is skipped with its reason.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli plugins reload --json"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "pane",
        summary: "Show, hide or move a pane a plugin contributed: what its rail button and a drag on its header do.",
        arguments: &[argument("pane", true, "The pane, as `<plugin id>/<pane id>` — `agent-tasks/board`.")],
        flags: &[
            switch("show", "Show it, building the plugin's own state the first time."),
            switch("hide", "Put it away."),
            option("side", "side", "Dock it to left, right, top or bottom."),
        ],
        examples: &[
            "quill-cli plugins pane agent-tasks/board --show",
            "quill-cli plugins pane agent-tasks/board --side bottom",
        ],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "tab",
        summary: "Open or close a plugin's own tab in the editing area: a tab with no file behind it.",
        arguments: &[argument("tab", true, "The tab, as `<plugin id>/<tab id>` — `agent-tasks/board`.")],
        flags: &[switch("open", "Open it, or show it if it is already open."), switch("close", "Close it.")],
        examples: &["quill-cli plugins tab agent-tasks/board --open"],
        local: false,
    },
    Command {
        area: "plugins",
        verb: "run",
        summary: "Run one of a plugin's own commands, down the same path its menu entry and its buttons take. `plugins show` lists them.",
        arguments: &[
            argument("id", true, "The plugin's id."),
            argument("command", true, "The command, as `plugins show` lists it."),
            rest("arguments", false, "The rest of the line, handed to the command as it stands."),
        ],
        examples: &[
            "quill-cli plugins run agent-tasks board --json",
            "quill-cli plugins run agent-tasks new-task Rewrite the importer",
            "quill-cli plugins run agent-tasks start task-27",
        ],
        flags: NO_FLAGS,
        local: false,
    },
    Command {
        area: "plugins",
        verb: "view",
        summary: "What a plugin's pane holds, as data rather than pixels: for Agent-Tasks the sprint, the four lanes, their counts and their cards. A screenshot cannot answer how many tickets are in progress; this can.",
        arguments: &[argument("id", true, "The plugin's id.")],
        flags: NO_FLAGS,
        examples: &["quill-cli plugins view agent-tasks --json"],
        local: false,
    },
    // ---------------------------------------------------------------------------------- the git
    Command {
        area: "git",
        verb: "status",
        summary: "What the machine's real git says about the project: the branch, whether a merge or rebase is unfinished, and what the last command returned, using the same credentials, SSH agent, configuration and hooks as the terminal.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli git status --json"],
        local: false,
    },
    Command {
        area: "git",
        verb: "actions",
        summary: "Everything on the Git menu, by the name `git action` takes.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli git actions --json"],
        local: false,
    },
    Command {
        area: "git",
        verb: "action",
        summary: "Run one of the entries on the Git menu. Git runs on a thread, so the answer says it was asked for, and --wait holds on for what came back.",
        arguments: &[argument("name", true, "The entry, such as commit, push, pull, fetch, branches or annotate.")],
        flags: &[
            option("path", "path", "The file it is about. The file that is showing when it is left out."),
            option("wait", "milliseconds", "Wait up to this long for git to answer before returning."),
        ],
        examples: &[
            "quill-cli git action fetch --wait 20000",
            "quill-cli git action annotate",
            "quill-cli git action show-history --path README.md",
        ],
        local: false,
    },
    // ----------------------------------------------------------------- every menu entry there is
    Command {
        area: "action",
        verb: "list",
        summary: "Every entry on every menu, with the name `action run` takes, the menu it is on, its keyboard shortcut and whether it can be used just now. A new menu entry appears here without anybody adding it. Ask for one menu with --menu and the answer is only that menu.",
        arguments: NO_ARGUMENTS,
        flags: &[option("menu", "name", "Only the entries on this menu, by the name it is shown under; submenus name their own rows. Several, comma-separated, for more than one. Every menu when it is left out.")],
        examples: &["quill-cli action list --json", "quill-cli action list --menu view --json"],
        local: false,
    },
    Command {
        area: "action",
        verb: "run",
        summary: "Run a menu entry by name. This is the way to reach something with no command of its own; the entries that would open a file chooser are refused, and the answer says which command to use instead.",
        arguments: &[argument("name", true, "The entry, as `action list` gives it, such as toggle-line-numbers.")],
        flags: &[option("path", "path", "The file or folder the entry is about, for the ones that take one.")],
        examples: &["quill-cli action run toggle-line-numbers", "quill-cli action run about"],
        local: false,
    },
    // ------------------------------------------------------------------------------ the project
    Command {
        area: "project",
        verb: "open",
        summary: "Show another folder in this window. What was open in the project being left is written down first.",
        arguments: &[argument("folder", true, "The folder to show.")],
        flags: NO_FLAGS,
        examples: &["quill-cli project open C:\\jason\\dev\\quill"],
        local: false,
    },
    Command {
        area: "project",
        verb: "recent",
        summary: "The projects that have been open, newest first.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli project recent --json"],
        local: false,
    },
    // ---------------------------------------------------------------------------------- the MCP server
    Command {
        area: "mcp",
        verb: "serve",
        summary: "Run the Model Context Protocol server, which is how an AI agent drives Quill. Over stdin and stdout by default, which is what an agent that launches it wants; over HTTP with `--transport http`.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("transport", "stdio|http", "How the client talks to it. `stdio` by default."),
            option("port", "number", "Which port to listen on, for `--transport http`. 7345 by default."),
            option("tools", "grouped|every", "One tool per area, or one tool per command. `grouped` by default."),
            option("instance", "which", "Which running Quill to drive, when several are running."),
        ],
        examples: &["quill-cli mcp serve", "quill-cli mcp serve --transport http --port 7345"],
        local: true,
    },
    Command {
        area: "mcp",
        verb: "status",
        summary: "What this Quill is doing about MCP: whether it is serving over HTTP, on which port, in which tool shape, how many tools that is, and where an agent's configuration should point.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli mcp status --json"],
        local: false,
    },
    Command {
        area: "mcp",
        verb: "install",
        summary: "Write Quill's MCP server into an agent's own configuration, so it is there next time the agent starts.",
        arguments: &[argument("client", true, "`claude`, `codex`, or `both`.")],
        flags: &[
            option("transport", "stdio|http", "Which way the agent should talk to it. `stdio` by default, which needs no port."),
            option("port", "number", "The port to point at, for `--transport http`."),
            option("scope", "user|project", "`user` for every project, `project` for this folder only. `user` by default."),
            option("name", "name", "What the server is called in the agent's configuration. `quill` by default."),
            switch("remove", "Take it out again rather than putting it in."),
        ],
        examples: &["quill-cli mcp install both", "quill-cli mcp install claude --scope project"],
        local: true,
    },
    Command {
        area: "mcp",
        verb: "config",
        summary: "Print the configuration to paste into an agent that has no button of its own: the JSON an `mcpServers` block wants, and the TOML Codex wants.",
        arguments: &[argument("client", false, "`claude` or `codex`. Both when it is left out.")],
        flags: &[
            option("transport", "stdio|http", "Which way to describe. `stdio` by default."),
            option("port", "number", "The port to name, for `--transport http`."),
            option("name", "name", "What to call the server. `quill` by default."),
        ],
        examples: &["quill-cli mcp config", "quill-cli mcp config codex --transport http"],
        local: true,
    },
    Command {
        area: "mcp",
        verb: "tools",
        summary: "The tools the MCP server offers, exactly as it would answer `tools/list`. This is how to see what an agent will be given, and how the cost of the two shapes is compared.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("tools", "grouped|every", "Which shape to print. `grouped` by default."),
            switch("count", "Print how many tools and how large the list is, rather than the list."),
        ],
        examples: &["quill-cli mcp tools --json", "quill-cli mcp tools --tools every --count"],
        local: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn given(pairs: Value) -> Map<String, Value> {
        pairs.as_object().expect("an object").clone()
    }

    /// The usage lines spell every flag with two dashes, so that is what a caller writing a request
    /// from the catalogue sends. Both spellings are one name.
    #[test]
    fn a_name_written_with_dashes_is_the_same_name_as_one_without() {
        assert_eq!(argument_name("--permanent"), "permanent");
        assert_eq!(argument_name("-permanent"), "permanent");
        assert_eq!(argument_name("permanent"), "permanent");
        assert_eq!(argument_name("--from-line"), "from-line", "only the leading dashes go");
        assert_eq!(argument_name("wait-for"), "wait-for");
    }

    #[test]
    fn equivalent_argument_styles_share_the_catalogue_name() {
        assert_eq!(canonical_argument_name("wait-for"), "wait-for");
        assert_eq!(canonical_argument_name("waitFor"), "wait-for");
        assert_eq!(canonical_argument_name("wait_for"), "wait-for");
        assert_eq!(canonical_argument_name("--fromLine"), "from-line");
    }

    #[test]
    fn the_dashes_come_off_every_key_and_the_values_are_kept() {
        let normalised = normalise_arguments(given(json!({
            "--permanent": true,
            "--from-line": 46,
            "path": "README.md",
        })));
        assert_eq!(normalised.get("permanent"), Some(&json!(true)));
        assert_eq!(normalised.get("from-line"), Some(&json!(46)));
        assert_eq!(normalised.get("path"), Some(&json!("README.md")));
        assert_eq!(normalised.len(), 3, "no key is left behind and none is duplicated");
    }

    /// A request carrying both spellings keeps the one the window reads, whichever order the object
    /// happened to be in.
    #[test]
    fn a_name_sent_both_ways_keeps_the_spelling_the_window_reads() {
        let normalised = normalise_arguments(given(json!({
            "--tail": 40,
            "tail": 3,
        })));
        assert_eq!(normalised.get("tail"), Some(&json!(3)));
        assert_eq!(normalised.len(), 1);
    }

    #[test]
    fn a_map_with_no_dashes_in_it_is_left_exactly_as_it_was() {
        let plain = given(json!({ "path": "a.rs", "permanent": true }));
        assert_eq!(normalise_arguments(plain.clone()), plain);
    }

    #[test]
    fn an_alias_argument_is_normalised_and_the_canonical_key_wins() {
        let normalised = normalise_arguments(given(json!({
            "waitFor": "ready",
            "wait_for": "wrong",
            "wait-for": "canonical",
        })));
        assert_eq!(normalised.get("wait-for"), Some(&json!("canonical")));
        assert_eq!(normalised.len(), 1);
    }

    #[test]
    fn a_neighbouring_command_hint_names_the_right_area_for_common_guesses() {
        let caret = find("editor caret").expect("editor caret");
        let run_add = find("run add").expect("run add");
        assert_eq!(argument_hint(caret, "to-column"), Some("editor select"));
        assert_eq!(argument_hint(caret, "tab"), Some("tab select"));
        assert_eq!(argument_hint(run_add, "wait-for"), Some("run output"));
    }

    /// The fault this closes: a value the command has no name for used to be dropped, and the
    /// command ran as though it had not been sent.
    #[test]
    fn a_name_the_command_does_not_have_is_reported_rather_than_dropped() {
        let open = find("tab open").expect("tab open");
        assert_eq!(
            unknown_arguments(open, &given(json!({ "path": "a.rs", "permanant": true }))),
            vec!["permanant".to_owned()],
            "a misspelling is unknown"
        );
        assert!(
            unknown_arguments(open, &given(json!({ "path": "a.rs", "--permanent": true })))
                .is_empty(),
            "the dashes are not what makes a name unknown"
        );
        assert!(
            unknown_arguments(open, &given(json!({ "path": "a.rs", "permanent": true }))).is_empty()
        );
    }

    /// Both a positional and a flag arrive under their own name, so both are known names.
    #[test]
    fn a_positional_and_a_flag_are_both_names_a_value_may_arrive_under() {
        let output = find("run output").expect("run output");
        let names = value_names(output);
        assert!(names.contains(&"name"), "the positional: {names:?}");
        assert!(names.contains(&"tail"), "a flag: {names:?}");
        assert!(unknown_arguments(output, &given(json!({ "name": "dev", "tail": 20 }))).is_empty());
    }

    /// Every command's own names have to be names it can be sent, or the refusal above would refuse
    /// a request the window itself reads.
    #[test]
    fn no_command_has_two_values_by_one_name() {
        for command in COMMANDS {
            let names = value_names(command);
            for (at, name) in names.iter().enumerate() {
                assert!(
                    !names[at + 1..].contains(name),
                    "{} has two values called {name}",
                    command.typed()
                );
                assert_eq!(
                    argument_name(name),
                    *name,
                    "{}'s {name} is written with the dashes a caller's key has them taken off",
                    command.typed()
                );
            }
        }
    }

    #[test]
    fn every_command_has_a_summary_and_an_example() {
        for command in COMMANDS {
            assert!(!command.summary.is_empty(), "{} has no summary", command.typed());
            assert!(!command.examples.is_empty(), "{} has no example", command.typed());
            for example in command.examples {
                assert!(
                    example.starts_with("quill-cli "),
                    "{}'s example should be a whole command line: {example}",
                    command.typed()
                );
            }
        }
    }

    #[test]
    fn no_two_commands_share_a_name() {
        let mut seen: Vec<String> = Vec::new();
        for command in COMMANDS {
            let name = command.wire();
            assert!(!seen.contains(&name), "two commands are called {name}");
            seen.push(name);
        }
    }

    #[test]
    fn names_are_lower_case_and_hyphenated() {
        for command in COMMANDS {
            for part in [command.area, command.verb] {
                assert!(
                    part.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                    "{} is not a lower case hyphenated name",
                    command.typed()
                );
            }
            for argument in command.arguments {
                assert!(
                    argument.name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                    "{}'s argument {} is not a lower case hyphenated name",
                    command.typed(),
                    argument.name
                );
            }
            for flag in command.flags {
                assert!(
                    flag.name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                    "{}'s flag {} is not a lower case hyphenated name",
                    command.typed(),
                    flag.name
                );
            }
        }
    }

    #[test]
    fn a_required_argument_never_follows_an_optional_one() {
        // Otherwise the optional one could not be left out, and the position of everything after it
        // would depend on whether it had been given.
        for command in COMMANDS {
            let mut seen_optional = false;
            for argument in command.arguments {
                if !argument.required {
                    seen_optional = true;
                } else {
                    assert!(
                        !seen_optional,
                        "{} has a required argument after an optional one",
                        command.typed()
                    );
                }
            }
        }
    }

    #[test]
    fn only_the_last_argument_takes_the_rest_of_the_line() {
        for command in COMMANDS {
            for (at, argument) in command.arguments.iter().enumerate() {
                if argument.rest {
                    assert_eq!(
                        at,
                        command.arguments.len() - 1,
                        "{}'s {} takes the rest of the line but is not last",
                        command.typed(),
                        argument.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_command_is_found_by_either_spelling_of_its_name() {
        assert_eq!(find("tab open").map(|c| c.wire()), Some("tab.open".to_owned()));
        assert_eq!(find("tab.open").map(|c| c.wire()), Some("tab.open".to_owned()));
        assert_eq!(find("status").map(|c| c.wire()), Some("status".to_owned()));
        assert!(find("tab").is_none(), "an area on its own is not a command");
        assert!(find("tab op").is_none(), "an abbreviation is not accepted");
    }

    #[test]
    fn file_verbs_guessed_under_editor_resolve_to_the_tab_commands() {
        for verb in ["open", "reload", "save", "close"] {
            assert_eq!(find(&format!("editor {verb}")).map(|command| command.wire()), Some(format!("tab.{verb}")));
        }
    }

    #[test]
    fn the_usage_line_shows_required_and_optional_apart() {
        let open = find("tab open").expect("tab open");
        assert_eq!(open.usage(), "quill-cli tab open <path> [--permanent]");
        let close = find("tab close").expect("tab close");
        assert_eq!(close.usage(), "quill-cli tab close [tab] [--discard]");
    }
}
