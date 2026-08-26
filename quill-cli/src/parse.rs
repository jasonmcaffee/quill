//! Turning what somebody typed into a request, against the catalogue.
//!
//! There is no argument-parsing library here. The catalogue already says what every command takes,
//! so parsing is walking the words once and asking it, and a library would mean writing the same
//! facts down twice — once for the parser and once for the help.
//!
//! ## The rules, in the order they are applied
//!
//! 1. The first one or two words are the command: `<area> <verb>`, or a single word for one of the
//!    six commands with no area. `area.verb` with a dot is accepted too, so what goes over the wire
//!    can also be typed.
//! 2. `--` ends flag parsing. Everything after it is a value, even if it starts with a dash.
//! 3. **Quill's own flags are recognised anywhere on the line** — the command's own first, then the
//!    global ones. Outside a text argument an unknown `--flag` is a usage error rather than a value,
//!    because a mistyped flag quietly treated as text is a command that did the wrong thing without
//!    saying so.
//! 4. Anything else fills the next positional argument. When that argument **takes the rest of the
//!    line** it takes every word left, and a `--flag` that is not one of Quill's own is part of the
//!    text — which is what lets `terminal send git log --oneline -n 5` send the whole shell command.
//!    A `--flag` that *is* one of Quill's is still Quill's, so `settings set appearance.font.size 20
//!    --json` means what it looks like. `--` turns even those into text:
//!    `terminal send -- curl --json https://example.com`.
//!
//! Both `--flag value` and `--flag=value` are accepted, because both are what people type.
//!
//! The single-letter forms — `-h`, `-n`, `-q`, `-V` — are read **only before the command name**.
//! After it, only the long spelling is Quill's. That is not tidiness: `git log -n 5` and `grep -q`
//! are ordinary things to put after `terminal send`, and a rule that claimed every `-n` on the line
//! would send the wrong command to somebody's shell.

use serde_json::{Map, Value};

use crate::catalogue::{self, Command};

/// The flags that mean the same thing whatever command they are on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Global {
    /// Which Quill to talk to: a process id, a port, or part of a project's path.
    pub instance: Option<String>,
    /// Print the whole reply as JSON rather than the sentence in it.
    pub json: bool,
    /// Print nothing on success.
    pub quiet: bool,
    /// How long to wait for an answer, in milliseconds.
    pub timeout: Option<u64>,
    /// Print the request that would be sent and send nothing.
    pub dry_run: bool,
    /// Print the help for this command, or for the CLI, and do nothing else.
    pub help: bool,
    /// Print the version and do nothing else.
    pub version: bool,
}

/// What a command line turned into.
#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub command: Option<&'static Command>,
    pub arguments: Map<String, Value>,
    pub global: Global,
}

/// Why a command line could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    /// The command it was about, so the caller can print that command's help rather than all of it.
    pub about: Option<&'static Command>,
}

impl Problem {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), about: None }
    }

    fn about(message: impl Into<String>, command: &'static Command) -> Self {
        Self { message: message.into(), about: Some(command) }
    }
}

/// The global flags, spelled out so that the help can print them and the parser can find them.
pub const GLOBAL_FLAGS: &[(&str, Option<&str>, &str)] = &[
    ("instance", Some("pid|port|path"), "Which Quill to talk to when several are running: its process id, its port, or part of its project's path."),
    ("json", None, "Print the whole reply as JSON. This is what a program or an agent should always pass."),
    ("quiet", None, "Print nothing when it worked. The exit code still says whether it did."),
    ("timeout", Some("milliseconds"), "How long to wait for an answer. 15000 by default. Lower it to fail fast; a command that waits for something of its own, such as `terminal read --wait-for`, is still waited out in full."),
    ("dry-run", None, "Print the command and the arguments that would be sent, and send nothing. Needs no running Quill."),
    ("no-color", None, "Never colour the output. NO_COLOR in the environment does the same."),
    ("help", None, "Print help for the command, or for the whole CLI, and do nothing else."),
    ("version", None, "Print the version and do nothing else."),
];

/// Read a command line. `words` is everything after the program's own name.
pub fn parse(words: &[String]) -> Result<Typed, Problem> {
    let mut global = Global::default();
    let mut rest: Vec<String> = Vec::new();

    // The command comes first, but `--help` and `--version` are allowed in front of it so that
    // `quill-cli --help` works, which is what everybody tries first.
    let mut at = 0;
    while at < words.len() {
        let word = &words[at];
        if !word.starts_with('-') {
            break;
        }
        match take_global(words, &mut at, &mut global)? {
            Taken::Yes => continue,
            Taken::No => {
                return Err(Problem::new(format!(
                    "{word} is not one of Quill's flags. Try `quill-cli --help`."
                )))
            }
        }
    }

    if at >= words.len() {
        return Ok(Typed { command: None, arguments: Map::new(), global });
    }

    // One word or two. `tab open` is two, `status` is one, and `tab.open` is one that holds both.
    let first = &words[at];
    let (command, mut next) = match catalogue::find(first) {
        Some(command) => (command, at + 1),
        None => {
            let joined = match words.get(at + 1) {
                Some(second) => format!("{first} {second}"),
                None => first.clone(),
            };
            match catalogue::find(&joined) {
                Some(command) => (command, at + 2),
                None => return Err(Problem::new(unknown(first, words.get(at + 1)))),
            }
        }
    };

    let mut arguments = Map::new();
    let mut filled = 0usize;
    let mut only_values = false;

    while next < words.len() {
        let word = words[next].clone();
        if !only_values && word == "--" {
            only_values = true;
            next += 1;
            continue;
        }
        // A positional that takes the rest of the line takes it whole once it has started, dashes
        // and all. It starts at the first word that is not a flag, so `terminal send --no-enter cd
        // ..` reads the switch and then sends `cd ..`, while `terminal send git log --oneline`
        // sends the whole shell command. A shell command that itself begins with a dash is reached
        // by putting `--` in front of it.
        let taking_rest = command
            .arguments
            .get(filled)
            .map(|argument| argument.rest)
            .unwrap_or(false);
        if !only_values && word.starts_with("--") {
            // The command's own flags are looked at first, so that `terminal read --timeout` is the
            // one that command documents rather than the CLI's own. The two are only ever the same
            // word where they mean nearly the same thing, and the nearer meaning is the right one.
            let (name, inline) = split_flag(&word);
            let known = command.flag(name);
            if known.is_none() {
                let mut here = next;
                if let Taken::Yes = take_global(words, &mut here, &mut global)? {
                    next = here;
                    continue;
                }
                // Not a flag of Quill's at all. Inside a text argument that is ordinary text — a
                // shell command's own switches have to survive — and anywhere else it is a mistake
                // worth saying so about.
                if !taking_rest {
                    return Err(Problem::about(
                        format!("{command} has no flag called --{name}.", command = command.typed()),
                        command,
                    ));
                }
            }
            let Some(flag) = known else {
                rest.push(word);
                next += 1;
                while next < words.len() {
                    let word = words[next].clone();
                    // Quill's flags go on being Quill's, even here.
                    if word.starts_with("--") {
                        let (name, _) = split_flag(&word);
                        if command.flag(name).is_some() {
                            break;
                        }
                        let mut here = next;
                        if let Taken::Yes = take_global(words, &mut here, &mut global)? {
                            next = here;
                            continue;
                        }
                    }
                    rest.push(word);
                    next += 1;
                }
                filled += 1;
                continue;
            };
            match flag.value {
                None => {
                    if inline.is_some() {
                        return Err(Problem::about(
                            format!("--{name} is a switch and takes no value."),
                            command,
                        ));
                    }
                    arguments.insert(name.to_owned(), Value::Bool(true));
                    next += 1;
                }
                Some(wants) => {
                    let value = match inline {
                        Some(value) => {
                            next += 1;
                            value.to_owned()
                        }
                        None => match words.get(next + 1) {
                            Some(value) => {
                                next += 2;
                                value.clone()
                            }
                            None => {
                                return Err(Problem::about(
                                    format!("--{name} needs a {wants} after it."),
                                    command,
                                ))
                            }
                        },
                    };
                    arguments.insert(name.to_owned(), Value::String(value));
                }
            }
            continue;
        }
        rest.push(word);
        next += 1;
        if taking_rest {
            // Everything left belongs to it, except a flag of Quill's own.
            while next < words.len() {
                let word = words[next].clone();
                if !only_values && word.starts_with("--") {
                    let (name, _) = split_flag(&word);
                    if command.flag(name).is_some() {
                        break;
                    }
                    let mut here = next;
                    if let Taken::Yes = take_global(words, &mut here, &mut global)? {
                        next = here;
                        continue;
                    }
                }
                rest.push(word);
                next += 1;
            }
        }
        filled += 1;
    }

    // The words that were not flags fill the positional arguments in order. The last one takes
    // whatever is left over when it is the kind that does.
    let mut values = rest.into_iter();
    for (at, argument) in command.arguments.iter().enumerate() {
        if argument.rest {
            let remaining: Vec<String> = values.by_ref().collect();
            if !remaining.is_empty() {
                arguments.insert(argument.name.to_owned(), Value::String(remaining.join(" ")));
            }
            break;
        }
        match values.next() {
            Some(value) => {
                arguments.insert(argument.name.to_owned(), Value::String(value));
            }
            None => {
                if argument.required && !global.help {
                    return Err(Problem::about(
                        format!(
                            "{} needs a {} as its {} argument.\n  {}",
                            command.typed(),
                            argument.name,
                            ordinal(at),
                            command.usage()
                        ),
                        command,
                    ));
                }
                break;
            }
        }
    }
    let left_over: Vec<String> = values.collect();
    if !left_over.is_empty() {
        return Err(Problem::about(
            format!(
                "{} takes {} argument{}, and was given {} more.\n  {}",
                command.typed(),
                command.arguments.len(),
                if command.arguments.len() == 1 { "" } else { "s" },
                left_over.len(),
                command.usage()
            ),
            command,
        ));
    }

    Ok(Typed { command: Some(command), arguments, global })
}

enum Taken {
    Yes,
    No,
}

/// Read one global flag at `at`, moving `at` past it. `Taken::No` when the word is not one.
fn take_global(words: &[String], at: &mut usize, global: &mut Global) -> Result<Taken, Problem> {
    let word = &words[*at];
    let short = match word.as_str() {
        "-h" => Some("--help"),
        "-n" => Some("--dry-run"),
        "-q" => Some("--quiet"),
        "-V" => Some("--version"),
        _ => None,
    };
    let word = short.unwrap_or(word.as_str());
    if !word.starts_with("--") {
        return Ok(Taken::No);
    }
    let (name, inline) = split_flag(word);
    let Some((_, wants, _)) = GLOBAL_FLAGS.iter().find(|(flag, _, _)| *flag == name) else {
        return Ok(Taken::No);
    };
    let value = match (wants, inline) {
        (None, Some(_)) => {
            return Err(Problem::new(format!("--{name} is a switch and takes no value.")))
        }
        (None, None) => {
            *at += 1;
            None
        }
        (Some(_), Some(value)) => {
            *at += 1;
            Some(value.to_owned())
        }
        (Some(kind), None) => match words.get(*at + 1) {
            Some(value) => {
                *at += 2;
                Some(value.clone())
            }
            None => return Err(Problem::new(format!("--{name} needs a {kind} after it."))),
        },
    };
    match name {
        "instance" => global.instance = value,
        "json" => global.json = true,
        "quiet" => global.quiet = true,
        "timeout" => {
            let text = value.unwrap_or_default();
            global.timeout = Some(text.trim().parse().map_err(|_| {
                Problem::new(format!("--timeout wants a number of milliseconds, not {text}."))
            })?);
        }
        // Colour is decided by `render`, which reads the same word out of the environment. Nothing
        // to keep here; the flag exists so that it is accepted and documented.
        "no-color" => {}
        "dry-run" => global.dry_run = true,
        "help" => global.help = true,
        "version" => global.version = true,
        _ => return Ok(Taken::No),
    }
    Ok(Taken::Yes)
}

/// `--name=value` split into its two halves, or `--name` with nothing after it.
fn split_flag(word: &str) -> (&str, Option<&str>) {
    let body = word.trim_start_matches('-');
    match body.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (body, None),
    }
}

fn ordinal(at: usize) -> &'static str {
    match at {
        0 => "first",
        1 => "second",
        2 => "third",
        _ => "next",
    }
}

/// What to say when there is no such command, with the nearest one that does exist.
fn unknown(first: &str, second: Option<&String>) -> String {
    let typed = match second {
        Some(second) if !second.starts_with('-') => format!("{first} {second}"),
        _ => first.to_owned(),
    };
    let mut message = format!("There is no command called `{typed}`.");
    if let Some(near) = nearest(&typed) {
        message.push_str(&format!(" Did you mean `{}`?", near.typed()));
    }
    if catalogue::areas().iter().any(|area| *area == first) {
        let verbs: Vec<String> =
            catalogue::in_area(first).iter().map(|command| command.verb.to_owned()).collect();
        message.push_str(&format!(" `{first}` holds: {}.", verbs.join(", ")));
    }
    message.push_str(" `quill-cli commands` lists them all.");
    message
}

/// The command whose name is closest to what was typed, when one is close enough to suggest.
///
/// A plain edit distance, and a cap of three changes so that a wild guess is not answered with a
/// confident suggestion. This is the only place the CLI guesses at what somebody meant, and it only
/// ever offers — `clig.dev` is clear that a tool must not quietly correct a command that changes
/// something.
fn nearest(typed: &str) -> Option<&'static Command> {
    let typed = typed.replace(' ', ".");
    let mut best: Option<(usize, &'static Command)> = None;
    for command in catalogue::COMMANDS {
        let distance = edits(&typed, &command.wire());
        if distance <= 3 && best.map(|(seen, _)| distance < seen).unwrap_or(true) {
            best = Some((distance, command));
        }
    }
    best.map(|(_, command)| command)
}

/// How many single character changes turn one string into the other.
fn edits(from: &str, to: &str) -> usize {
    let from: Vec<char> = from.chars().collect();
    let to: Vec<char> = to.chars().collect();
    let mut row: Vec<usize> = (0..=to.len()).collect();
    for (i, a) in from.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, b) in to.iter().enumerate() {
            let cost = usize::from(a != b);
            let next = (row[j] + 1).min(row[j + 1] + 1).min(previous + cost);
            previous = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[to.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    fn parsed(line: &str) -> Typed {
        parse(&words(line)).unwrap_or_else(|problem| panic!("{line}: {}", problem.message))
    }

    #[test]
    fn an_area_and_a_verb_find_the_command() {
        let typed = parsed("tab open README.md");
        assert_eq!(typed.command.map(|c| c.wire()), Some("tab.open".to_owned()));
        assert_eq!(typed.arguments["path"], Value::String("README.md".to_owned()));
    }

    #[test]
    fn a_command_with_no_area_is_one_word() {
        assert_eq!(parsed("status").command.map(|c| c.wire()), Some("status".to_owned()));
    }

    #[test]
    fn the_wire_spelling_can_be_typed_too() {
        assert_eq!(parsed("tab.open a.md").command.map(|c| c.wire()), Some("tab.open".to_owned()));
    }

    #[test]
    fn a_switch_becomes_a_true_and_an_option_becomes_its_value() {
        let typed = parsed("tab open notes.md --permanent");
        assert_eq!(typed.arguments["permanent"], Value::Bool(true));
        let typed = parsed("explorer files --limit 20");
        assert_eq!(typed.arguments["limit"], Value::String("20".to_owned()));
        let typed = parsed("explorer files --limit=20");
        assert_eq!(typed.arguments["limit"], Value::String("20".to_owned()));
    }

    #[test]
    fn a_dry_run_is_read_by_either_of_its_names() {
        assert!(parsed("tab open a.md --dry-run").global.dry_run);
        assert!(parsed("-n tab open a.md").global.dry_run);
        assert!(!parsed("tab open a.md").global.dry_run);
    }

    #[test]
    fn global_flags_are_read_before_or_after_the_command() {
        assert!(parsed("--json status").global.json);
        assert!(parsed("status --json").global.json);
        assert_eq!(parsed("--instance 4212 status").global.instance.as_deref(), Some("4212"));
        assert_eq!(parsed("status --timeout 500").global.timeout, Some(500));
    }

    #[test]
    fn a_text_argument_takes_the_rest_of_the_line_including_dashes() {
        // This is what makes the terminal usable: the shell's own flags must not be read as Quill's.
        let typed = parsed("terminal send git log --oneline -n 5");
        assert_eq!(typed.arguments["text"], Value::String("git log --oneline -n 5".to_owned()));
        assert!(!typed.global.json, "a flag the CLI does not have is not one of Quill's");
    }

    #[test]
    fn quills_own_flags_are_still_quills_after_a_text_argument() {
        // Found by driving a real window: every example tells an agent to pass --json, and a text
        // argument that ate it turned `settings set appearance.font.size 20 --json` into a request
        // to set the size to the string "20 --json". A flag Quill has is Quill's wherever it sits.
        let typed = parsed("settings set appearance.font.size 20 --json");
        assert_eq!(typed.arguments["value"], Value::String("20".to_owned()));
        assert!(typed.global.json);

        let typed = parsed("window message the terminal has run git status --json");
        assert_eq!(
            typed.arguments["text"],
            Value::String("the terminal has run git status".to_owned())
        );
        assert!(typed.global.json);

        let typed = parsed("terminal send git status --no-enter");
        assert_eq!(typed.arguments["text"], Value::String("git status".to_owned()));
        assert_eq!(typed.arguments["no-enter"], Value::Bool(true));
    }

    #[test]
    fn a_double_dash_makes_even_quills_own_flags_into_text() {
        let typed = parsed("terminal send -- curl --json https://example.com");
        assert_eq!(
            typed.arguments["text"],
            Value::String("curl --json https://example.com".to_owned())
        );
        assert!(!typed.global.json, "after -- nothing is a flag");
    }

    #[test]
    fn a_text_argument_that_begins_with_an_unknown_flag_is_still_text() {
        let typed = parsed("terminal send --oneline is not a command but it is text");
        assert_eq!(
            typed.arguments["text"],
            Value::String("--oneline is not a command but it is text".to_owned())
        );
    }

    #[test]
    fn quills_own_flags_go_before_the_text() {
        let typed = parsed("terminal send --no-enter cd ..");
        assert_eq!(typed.arguments["text"], Value::String("cd ..".to_owned()));
        assert_eq!(typed.arguments["no-enter"], Value::Bool(true));
    }

    #[test]
    fn a_double_dash_ends_the_flags() {
        let typed = parsed("settings set appearance.font.family -- Courier New");
        assert_eq!(typed.arguments["key"], Value::String("appearance.font.family".to_owned()));
        assert_eq!(typed.arguments["value"], Value::String("Courier New".to_owned()));
    }

    #[test]
    fn a_missing_required_argument_says_which_one_and_shows_the_usage() {
        let problem = parse(&words("tab open")).expect_err("should refuse");
        assert!(problem.message.contains("path"), "{}", problem.message);
        assert!(problem.message.contains("quill-cli tab open <path>"), "{}", problem.message);
        assert_eq!(problem.about.map(|c| c.wire()), Some("tab.open".to_owned()));
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_taken_as_text() {
        let problem = parse(&words("tab open a.md --purple")).expect_err("should refuse");
        assert!(problem.message.contains("--purple"), "{}", problem.message);
    }

    #[test]
    fn too_many_arguments_are_refused() {
        let problem = parse(&words("tab show 1 2")).expect_err("should refuse");
        assert!(problem.message.contains("was given 1 more"), "{}", problem.message);
    }

    #[test]
    fn an_unknown_command_suggests_the_nearest_one() {
        let problem = parse(&words("tab opne x")).expect_err("should refuse");
        assert!(problem.message.contains("tab open"), "{}", problem.message);
    }

    #[test]
    fn an_area_on_its_own_lists_what_is_under_it() {
        let problem = parse(&words("explorer")).expect_err("should refuse");
        assert!(problem.message.contains("show"), "{}", problem.message);
        assert!(problem.message.contains("filter"), "{}", problem.message);
    }

    #[test]
    fn help_and_version_are_read_with_no_command_at_all() {
        assert!(parsed("--help").global.help);
        assert!(parsed("-h").global.help);
        assert!(parsed("--version").global.version);
        assert!(parsed("-V").global.version);
        assert!(parsed("tab open --help").global.help, "help about one command");
    }

    #[test]
    fn help_about_a_command_does_not_need_that_commands_arguments() {
        let typed = parsed("tab open --help");
        assert_eq!(typed.command.map(|c| c.wire()), Some("tab.open".to_owned()));
        assert!(typed.global.help);
    }

    #[test]
    fn every_example_in_the_catalogue_parses() {
        // The examples are what an agent copies, so an example that does not parse is worse than no
        // example. Quoting is undone first, because a shell would have done that.
        for command in catalogue::COMMANDS {
            for example in command.examples {
                let words = shell_words(example);
                let typed = parse(&words[1..]).unwrap_or_else(|problem| {
                    panic!("the example `{example}` does not parse: {}", problem.message)
                });
                assert_eq!(
                    typed.command.map(|c| c.wire()),
                    Some(command.wire()),
                    "the example `{example}` runs a different command"
                );
            }
        }
    }

    /// Split a command line the way a shell would, honouring double quotes.
    fn shell_words(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut word = String::new();
        let mut quoted = false;
        for character in line.chars() {
            match character {
                '"' => quoted = !quoted,
                c if c.is_whitespace() && !quoted => {
                    if !word.is_empty() {
                        out.push(std::mem::take(&mut word));
                    }
                }
                c => word.push(c),
            }
        }
        if !word.is_empty() {
            out.push(word);
        }
        out
    }
}
