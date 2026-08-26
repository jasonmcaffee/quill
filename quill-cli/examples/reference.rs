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
            // The heading and its paragraph live in the catalogue rather than here, because the MCP
            // tools read the same two lines: an area's description is what an agent is given
            // instead of this document, and a second copy would be a second copy to fall behind.
            out.push_str(&format!(
                "## {}\n\n{}\n\n",
                catalogue::area_title(command.area),
                catalogue::area_note(command.area)
            ));
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
