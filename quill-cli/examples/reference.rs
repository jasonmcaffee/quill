//! Write the reference half of `quill-cli/docs/commands.md` from the catalogue.
//!
//! Run it with `cargo run -p quill-cli --example reference`.
//!
//! The document is **mostly written by hand**: the opening, the recipes and the notes are prose that
//! nothing generates. What this replaces is only the part between the two markers — one section a
//! command, with its usage line, its summary, its arguments, its flags and its examples. That half
//! is generated because it is the half a test insists on being correct: a command with no section,
//! or a section whose usage line no longer matches, fails `documentation.rs`, and the way to fix it
//! is to run this rather than to retype it.
//!
//! Nothing else in the file is touched, and the file is only rewritten when it would change, so
//! running it on an up to date checkout leaves the working tree clean.

use std::path::PathBuf;

use quill_cli::catalogue::{self, Command};

const BEGIN: &str = "<!-- begin generated reference -->";
const END: &str = "<!-- end generated reference -->";

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/commands.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|problem| {
        eprintln!("could not read {}: {problem}", path.display());
        std::process::exit(1);
    });
    let (Some(begins), Some(ends)) = (existing.find(BEGIN), existing.find(END)) else {
        eprintln!(
            "{} has no `{BEGIN}` and `{END}` markers, so there is nowhere to write the reference.",
            path.display()
        );
        std::process::exit(1);
    };
    let wanted = format!(
        "{}{BEGIN}\n\n{}\n{}",
        &existing[..begins],
        reference(),
        &existing[ends..]
    );
    if wanted == existing {
        println!("{} is already up to date.", path.display());
        return;
    }
    std::fs::write(&path, &wanted).expect("write the reference");
    println!("Wrote {} commands into {}.", catalogue::COMMANDS.len(), path.display());
}

/// One section for every command, grouped by area in the order the catalogue lists them.
fn reference() -> String {
    let mut out = String::new();
    let mut last: Option<&str> = None;
    for command in catalogue::COMMANDS {
        if last != Some(command.area) {
            out.push_str(&format!("## {}\n\n{}\n\n", area_title(command.area), area_note(command.area)));
            last = Some(command.area);
        }
        out.push_str(&section(command));
    }
    out
}

fn section(command: &Command) -> String {
    let mut out = format!("### {}\n\n```\n{}\n```\n\n{}\n\n", command.typed(), command.usage(), command.summary);
    if !command.arguments.is_empty() {
        for argument in command.arguments {
            out.push_str(&format!(
                "- `{}`{} — {}{}\n",
                argument.name,
                if argument.required { "" } else { " (optional)" },
                argument.help,
                if argument.rest { " Everything after it on the line belongs to it." } else { "" }
            ));
        }
        out.push('\n');
    }
    for flag in command.flags {
        let spelled = match flag.value {
            Some(value) => format!("--{} <{value}>", flag.name),
            None => format!("--{}", flag.name),
        };
        out.push_str(&format!("- `{spelled}` — {}\n", flag.help));
    }
    if !command.flags.is_empty() {
        out.push('\n');
    }
    out.push_str("```sh\n");
    for example in command.examples {
        out.push_str(example);
        out.push('\n');
    }
    out.push_str("```\n\n");
    if command.local {
        out.push_str("Answered by the CLI itself; no Quill needs to be running.\n\n");
    }
    out
}

fn area_title(area: &str) -> &str {
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
        other => other,
    }
}

fn area_note(area: &str) -> &str {
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
        _ => "",
    }
}
