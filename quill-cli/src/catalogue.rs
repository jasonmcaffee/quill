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
//! area: `tab`, `editor`, `terminal`, `explorer`, `modal`, `settings`, `plugins`, `git`, `window`,
//! `project`, `action`. Six commands have no area, because they are about the CLI or about a whole
//! Quill: `status`, `instances`, `launch`, `quit`, `commands` and `version`.
//!
//! Names are lower case and hyphenated — `save-as`, `go-to-file`, `find-in-files` — and never
//! abbreviated to something a reader would have to learn.
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
        summary: "Everything about the window in one answer: the project, the tabs, the panes, the terminal, the modal that is open, the settings and git.",
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
        summary: "What version this command line tool is. The version of the Quill editor it is talking to is in `status`, and `action run about` puts it in the status bar.",
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
        summary: "Close a tab. Closing the last one leaves an empty untitled tab rather than no tab at all.",
        arguments: &[argument("tab", false, "Its number, name or path. The tab that is showing when it is left out.")],
        flags: NO_FLAGS,
        examples: &["quill-cli tab close", "quill-cli tab close notes.md"],
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
        verb: "preview",
        summary: "Read the preview of the tab that is showing: a Markdown page as plain text with where its pictures and diagrams are, or, for a Mermaid file, what the diagram came out as.",
        arguments: NO_ARGUMENTS,
        flags: NO_FLAGS,
        examples: &["quill-cli editor preview --json"],
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
        arguments: &[argument("name", true, "go-to-file, find-in-files, settings, new-file or rename.")],
        flags: &[
            option("query", "text", "Type this into the modal's box as it opens."),
            option("path", "path", "The folder a new file goes in, or the file being renamed. Needed by new-file and rename."),
            option("page", "name", "Which page the Settings modal shows: appearance, editor, plugins or terminal."),
        ],
        examples: &[
            "quill-cli modal open go-to-file --query mdrs",
            "quill-cli modal open find-in-files --query \"fn main\"",
            "quill-cli modal open settings --page terminal",
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
        assert_eq!(close.usage(), "quill-cli tab close [tab]");
    }
}
