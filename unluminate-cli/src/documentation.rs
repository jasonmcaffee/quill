//! The rule that keeps the documentation honest.
//!
//! `task-1661` asks that any new feature come with a CLI command **and be documented**. A rule
//! nothing enforces is a rule that lasts until the first busy afternoon, so this is the enforcement:
//! every command in the catalogue must have a heading of its own in `unluminate-cli/docs/commands.md`,
//! with its usage line under it, and nothing may be documented that no longer exists.
//!
//! It fails loudly and says exactly what to add, because the person it is talking to has just
//! written the command and is about to write the paragraph.

use crate::catalogue::{self, Command};

/// Read the written reference. It sits beside this crate, so its path is relative to the manifest.
fn reference() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/commands.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|problem| panic!("could not read {}: {problem}", path.display()))
}

/// The heading a command is documented under: `### tab open`.
fn heading(command: &Command) -> String {
    format!("### {}", command.typed())
}

#[test]
fn every_command_has_a_section_in_the_written_reference() {
    let text = reference();
    let missing: Vec<String> = catalogue::COMMANDS
        .iter()
        .filter(|command| !text.contains(&heading(command)))
        .map(|command| heading(command))
        .collect();
    assert!(
        missing.is_empty(),
        "unluminate-cli/docs/commands.md is missing {} command{}:\n{}\n\
         Add a `### <area> <verb>` section for each, with its usage line and an example.",
        missing.len(),
        if missing.len() == 1 { "" } else { "s" },
        missing.join("\n")
    );
}

#[test]
fn every_commands_usage_line_is_written_out_where_it_is_documented() {
    // The usage line is the one thing a reader copies, so it has to be the real one rather than a
    // remembered one. It is generated from the catalogue, so this is checking that the paragraph
    // was written against the command as it is now.
    let text = reference();
    let wrong: Vec<String> = catalogue::COMMANDS
        .iter()
        .filter(|command| !text.contains(&command.usage()))
        .map(|command| format!("{}\n    expected: {}", command.typed(), command.usage()))
        .collect();
    assert!(
        wrong.is_empty(),
        "unluminate-cli/docs/commands.md does not carry the current usage line for:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn nothing_is_documented_that_no_longer_exists() {
    let text = reference();
    let stale: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("### "))
        .map(|line| line.trim_start_matches("### ").trim())
        .filter(|name| catalogue::find(name).is_none())
        .collect();
    assert!(
        stale.is_empty(),
        "unluminate-cli/docs/commands.md documents commands that do not exist: {}",
        stale.join(", ")
    );
}

#[test]
fn the_reference_leads_with_what_an_agent_needs_before_anything_else() {
    // The document's whole purpose is to be handed to an agent, so the first thing in it has to be
    // the two facts that make every other line usable: how to find out what commands exist, and
    // that a program should always ask for JSON.
    let text = reference();
    let opening: String = text.lines().take(60).collect::<Vec<_>>().join("\n");
    assert!(opening.contains("--json"), "the opening should say to pass --json");
    assert!(
        opening.contains("unluminate-cli commands"),
        "the opening should say how to list the commands"
    );
}
