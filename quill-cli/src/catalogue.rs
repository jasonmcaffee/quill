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
//! area: `tab`, `pane`, `editor`, `terminal`, `explorer`, `modal`, `settings`, `plugins`, `git`,
//! `window`, `project`, `action`. Six commands have no area, because they are about the CLI or about a whole
//! Quill: `status`, `instances`, `launch`, `quit`, `commands` and `version`.
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

/// Find a command by what a person typed or by what goes over the wire.
///
/// Both spellings are accepted from both sides, so `quill-cli tab.open` works and a program that
/// only has the typed name can send it. An abbreviation is not accepted: `clig.dev` asks for
/// explicit aliases rather than unique prefixes, because a prefix that is unique today stops being
/// unique when a command is added and somebody's script quietly starts doing something else.
pub fn find(name: &str) -> Option<&'static Command> {
    let wanted = name.trim().replace(' ', ".");
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
        "tab" => "tab — the files that are open",
        "pane" => "pane — the editing area split into panes",
        "editor" => "editor — the text in the tab that is showing",
        "highlight" => "highlight — the passages marked in the project's files",
        "terminal" => "terminal — the shells along the bottom",
        "explorer" => "explorer — the file tree down the left",
        "modal" => "modal — every dialog, driven the same way",
        "settings" => "settings — Edit -> Settings, by the names in the settings file",
        "plugins" => "plugins — the languages Quill colours",
        "git" => "git — the Git menu",
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
        "tab" => "A tab holds a file. A relative path is resolved against the project folder, and every reply says which absolute path it used.",
        "pane" => "The editing area can be split into panes side by side, each with its own tabs, which is IntelliJ's split view. `pane split` moves the tab that is showing into a new pane on the right — it moves rather than copies, because two tabs on one file would be two documents over one path. A pane holding only that tab keeps it and the new pane opens empty, ready for the next file: opening a file always lands in the pane that has the keyboard.",
        "editor" => "These are about the tab that is showing. Lines and columns count from 1, which is what the status bar shows.",
        "highlight" => "A highlight is a colour behind a passage of text. It stays there until it is cleared, in this file and next time the project is opened, and it moves with the text as the file is edited. These work on a file whether it is open or not, so `highlight apply` can mark twenty passages across twenty files in one call.",
        "terminal" => "`terminal send` types into the shell and presses Enter; `terminal read --wait-for` is how to wait for what it did.",
        "explorer" => "`explorer files` is the list Quill searches, which leaves out `target`, `node_modules` and `__pycache__`.",
        "modal" => "One set of commands drives all of them: open it, type in it, read its results, choose a row, accept or cancel. A modal added to Quill later is driven with these same commands.",
        "settings" => "The names are the ones in Quill's own `settings.conf`, so there is one vocabulary rather than two. A change takes effect at once, in every tab, and is written to the file.",
        "plugins" => "A plugin describes a language: its extensions, its keywords and a colour per kind of token. Nothing in one is executed and nothing is fetched over a network.",
        "git" => "Git runs on a thread, so an action is asked for and `git status` says what came back. `--wait` holds the answer open until it has.",
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
        summary: "Everything about the window in one answer: its version and build date, the project, the tabs, the panes, the terminal, the modal that is open, the settings and git.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli status --json"],
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
        verb: "definition",
        summary: "Where the word at the caret is defined. Prints every candidate the project holds, best first, and --open goes to the best one. A file whose language has not said what a definition looks like has none, which is stated rather than guessed at.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("offset", "bytes", "Ask about this position in the file rather than about the caret."),
            option("line", "number", "Ask about this line, counting from 1."),
            option("column", "number", "The column on that line. 1 when it is left out."),
            switch("open", "Go to the best candidate, opening its file as a tab."),
        ],
        examples: &[
            "quill-cli editor definition --json",
            "quill-cli editor definition --line 42 --column 9 --open",
        ],
        local: false,
    },
    Command {
        area: "editor",
        verb: "references",
        summary: "Every place a name is used across the project: the file, the line, the column and whether it is code or a word inside a comment or a string. Reads the open tabs as they stand and everything else from the disk.",
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
        summary: "Rename the word at the caret everywhere it is used. Prints the change set without --apply, so a script can look before it leaps; --apply edits the open tabs as documents, one undo step each, and rewrites the closed files on the disk.",
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
        summary: "The names the word being typed at the caret could become, best first: the same list the popup shows, with what each row is and where it came from. Inside an import it is what can be imported instead — the project's files in a module specifier, and what a module exports between the braces — and there it answers with nothing typed. --choose applies one of them exactly as Enter would.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("offset", "bytes", "Ask about this position in the file rather than about the caret."),
            option("line", "number", "Ask about this line, counting from 1."),
            option("column", "number", "The column on that line. 1 when it is left out."),
            option("limit", "number", "Print at most this many rows. All of them when it is left out."),
            option("choose", "name", "Apply this row to the word being typed, as Enter would. It has to be one of the names offered."),
        ],
        examples: &[
            "quill-cli editor complete --json",
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
        summary: "Show one of the terminal tabs.",
        arguments: &[argument("index", true, "Its number, counting from 0.")],
        flags: NO_FLAGS,
        examples: &["quill-cli terminal select 1"],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "close",
        summary: "Close a terminal tab. Closing the last one puts the terminal away.",
        arguments: &[argument("index", false, "Its number. The tab that is showing when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli terminal close"],
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
        summary: "Send a command to the shell in the terminal tab that is showing. Enter is pressed for you unless you say not to.",
        arguments: &[rest("text", false, "The command. Everything after the verb is taken as the command, so it needs no quotes.")],
        flags: &[
            switch("no-enter", "Type the text and leave it on the prompt without running it."),
            option("key", "name", "Send a key instead of text: enter, tab, escape, up, down, left, right, backspace, ctrl-c, ctrl-d, ctrl-l."),
        ],
        examples: &[
            "quill-cli terminal send git status",
            "quill-cli terminal send --key ctrl-c",
            "quill-cli terminal send --no-enter cd ..",
        ],
        local: false,
    },
    Command {
        area: "terminal",
        verb: "read",
        summary: "Read what the terminal tab that is showing has on its screen.",
        arguments: NO_ARGUMENTS,
        flags: &[
            option("lines", "number", "Only the last so many lines."),
            option("wait-for", "text", "Wait until this text is on the screen before answering, which is how to wait for a command to finish."),
            option("timeout", "milliseconds", "How long to wait for --wait-for. 10000 by default."),
        ],
        examples: &[
            "quill-cli terminal read --lines 20",
            "quill-cli terminal read --wait-for \"$\" --timeout 15000",
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
    // ---------------------------------------------------------------------------------- the git
    Command {
        area: "git",
        verb: "status",
        summary: "What git says about the project: the branch, whether a merge or a rebase is unfinished, and what the last command it was asked for came back with.",
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
        summary: "Every entry on every menu, with the name `action run` takes, the menu it is on, its keyboard shortcut and whether it can be used just now. A new menu entry appears here without anybody adding it.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli action list --json"],
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
    fn the_usage_line_shows_required_and_optional_apart() {
        let open = find("tab open").expect("tab open");
        assert_eq!(open.usage(), "quill-cli tab open <path> [--permanent]");
        let close = find("tab close").expect("tab close");
        assert_eq!(close.usage(), "quill-cli tab close [tab] [--discard]");
    }
}
