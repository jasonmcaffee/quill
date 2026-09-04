//! The help, and the same thing as data.
//!
//! Both are printed from the catalogue, so there is nothing here that can fall behind the commands.
//! `clig.dev` asks for the examples to come first and for the common things to be listed before the
//! rare ones, and for `-h` on a subcommand to show that subcommand's help rather than all of it.
//!
//! [`as_json`] is the same list as data, which is what `unluminate-cli commands --json` prints. That is
//! the form meant for a program or an agent: it can be read once at the start of a session and
//! every command written from it without guessing at a spelling.

use serde_json::{json, Value};

use crate::catalogue::{self, Command};
use crate::parse::GLOBAL_FLAGS;

/// The help for the whole CLI.
pub fn overall() -> String {
    let mut out = String::new();
    out.push_str("unluminate-cli — drive a running Unluminate window from the command line.\n\n");
    out.push_str("Usage:\n  unluminate-cli [flags] <area> <verb> [arguments]\n\n");
    out.push_str("Examples:\n");
    for example in [
        "unluminate-cli status --json",
        "unluminate-cli tab open README.md",
        "unluminate-cli editor view preview",
        "unluminate-cli terminal send git status",
        "unluminate-cli modal open go-to-file --query mdrs",
        "unluminate-cli settings set appearance.font.size 20",
        "unluminate-cli window screenshot _agent_output/after.png",
    ] {
        out.push_str(&format!("  {example}\n"));
    }
    out.push_str("\nCommands with no area:\n");
    for command in catalogue::in_area("") {
        out.push_str(&format!("  {:<22}{}\n", command.verb, first_sentence(command.summary)));
    }
    out.push_str("\nAreas:\n");
    for area in catalogue::areas() {
        let verbs: Vec<&str> =
            catalogue::in_area(area).iter().map(|command| command.verb).collect();
        out.push_str(&format!("  {:<22}{}\n", area, verbs.join(", ")));
    }
    out.push_str("\nFlags that work on every command:\n");
    for (name, value, help) in GLOBAL_FLAGS {
        let spelled = match value {
            Some(value) => format!("--{name} <{value}>"),
            None => format!("--{name}"),
        };
        out.push_str(&format!("  {spelled:<26}{help}\n"));
    }
    out.push_str(
        "\n`unluminate-cli commands --json` prints every command as data, which is what to give a\n\
         program or an agent. `unluminate-cli <area> <verb> --help` explains one command.\n\
         The written documentation is in unluminate-cli/docs/commands.md.\n",
    );
    out
}

/// The help for one command.
pub fn for_command(command: &Command) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n\n{}\n\n", command.usage(), command.summary));
    if !command.examples.is_empty() {
        out.push_str("Examples:\n");
        for example in command.examples {
            out.push_str(&format!("  {example}\n"));
        }
        out.push('\n');
    }
    if !command.arguments.is_empty() {
        out.push_str("Arguments:\n");
        for argument in command.arguments {
            let name = if argument.required {
                format!("<{}>", argument.name)
            } else {
                format!("[{}]", argument.name)
            };
            out.push_str(&format!("  {name:<20}{}\n", argument.help));
        }
        out.push('\n');
    }
    if !command.flags.is_empty() {
        out.push_str("Flags:\n");
        for flag in command.flags {
            let spelled = match flag.value {
                Some(value) => format!("--{} <{value}>", flag.name),
                None => format!("--{}", flag.name),
            };
            out.push_str(&format!("  {spelled:<26}{}\n", flag.help));
        }
        out.push('\n');
    }
    if command.local {
        out.push_str("Answered by the CLI itself; no Unluminate needs to be running.\n");
    }
    out
}

/// Every command as data, or one of them.
pub fn as_json(only: Option<&str>) -> Value {
    let chosen: Vec<&'static Command> = match only {
        Some(name) => catalogue::find(name).into_iter().collect(),
        None => catalogue::COMMANDS.iter().collect(),
    };
    json!({
        "usage": "unluminate-cli [flags] <area> <verb> [arguments]",
        "globalFlags": GLOBAL_FLAGS
            .iter()
            .map(|(name, value, help)| json!({ "name": name, "value": value, "help": help }))
            .collect::<Vec<Value>>(),
        "commands": chosen.iter().map(|command| one(command)).collect::<Vec<Value>>(),
    })
}

fn one(command: &Command) -> Value {
    json!({
        "name": command.typed(),
        "wire": command.wire(),
        "area": command.area,
        "verb": command.verb,
        "usage": command.usage(),
        "summary": command.summary,
        "local": command.local,
        "arguments": command.arguments.iter().map(|argument| json!({
            "name": argument.name,
            "required": argument.required,
            "takesTheRestOfTheLine": argument.rest,
            "help": argument.help,
        })).collect::<Vec<Value>>(),
        "flags": command.flags.iter().map(|flag| json!({
            "name": flag.name,
            "value": flag.value,
            "help": flag.help,
        })).collect::<Vec<Value>>(),
        "examples": command.examples,
    })
}

/// The first sentence of a summary, for the one-line listing.
fn first_sentence(summary: &str) -> &str {
    match summary.find(". ") {
        Some(at) => &summary[..=at],
        None => summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_help_names_every_area_and_every_command_with_no_area() {
        let help = overall();
        for area in catalogue::areas() {
            assert!(help.contains(area), "the help should name the {area} area");
        }
        for command in catalogue::in_area("") {
            assert!(help.contains(command.verb), "the help should name {}", command.verb);
        }
    }

    #[test]
    fn a_commands_help_leads_with_its_examples() {
        let command = catalogue::find("terminal send").expect("terminal send");
        let help = for_command(command);
        let examples = help.find("Examples:").expect("examples");
        assert!(help.find("Flags:").expect("flags") > examples, "examples come first");
        assert!(help.contains("unluminate-cli terminal send git status"));
    }

    #[test]
    fn the_data_form_holds_every_command() {
        let value = as_json(None);
        let commands = value["commands"].as_array().expect("an array");
        assert_eq!(commands.len(), catalogue::COMMANDS.len());
        let named: Vec<&str> =
            commands.iter().map(|command| command["name"].as_str().unwrap()).collect();
        assert!(named.contains(&"tab open"));
        assert!(named.contains(&"status"));
    }

    #[test]
    fn the_data_form_can_be_asked_for_one_command() {
        let value = as_json(Some("modal open"));
        let commands = value["commands"].as_array().expect("an array");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0]["wire"], json!("modal.open"));
    }
}
