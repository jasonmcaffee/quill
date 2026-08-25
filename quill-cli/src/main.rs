//! `quill-cli`: the program a person or an agent types at.
//!
//! It does four things and no more: read the command line against the catalogue, find the Quill to
//! talk to, send the request, and print the reply. Nothing about what a command *means* is here —
//! that is the window's, which is what makes the CLI and a three line Python script equally
//! complete ways in.
//!
//! ## What it prints, and where
//!
//! The reply goes to standard output and everything else goes to standard error, so
//! `quill-cli editor text > file.md` writes the document and not a sentence about it. With `--json`
//! the whole reply is printed as JSON; without it, the sentence the window wrote, and then the
//! text or the lines the reply carries. `--quiet` prints nothing when it worked.
//!
//! ## What it exits with
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | It worked. |
//! | 1 | Quill refused it: no such file, no such tab, nothing to undo. |
//! | 2 | The command line was wrong: no such command, no such flag, a missing argument. |
//! | 3 | No Quill is running, or the one named could not be reached. |
//! | 4 | Several Quills are running and none was named with `--instance`. |
//! | 5 | Quill was reached but did not answer in time. |
//!
//! The split is the one a script cares about: 2 is the caller's mistake, 1 is Quill's answer, and
//! 3, 4 and 5 are about the connection rather than about the command.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

use quill_cli::catalogue::Command;
use quill_cli::client::{self, Unreachable, DEFAULT_LAUNCH_TIMEOUT, DEFAULT_TIMEOUT};
use quill_cli::instances::Instance;
use quill_cli::parse::{self, Global, Typed};
use quill_cli::protocol::{code, Reply};
use quill_cli::{help, VERSION};

const OK: i32 = 0;
const REFUSED: i32 = 1;
const USAGE: i32 = 2;
const NOT_RUNNING: i32 = 3;
const SEVERAL: i32 = 4;
const TIMED_OUT: i32 = 5;

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&words));
}

/// Read the command line and do what it says. Split out from `main` so that it returns the exit
/// code rather than taking the process down, which is what makes it testable.
fn run(words: &[String]) -> i32 {
    let typed = match parse::parse(words) {
        Ok(typed) => typed,
        Err(problem) => {
            complain(&problem.message, &Global::default());
            if let Some(command) = problem.about {
                eprintln!();
                eprint!("{}", help::for_command(command));
            }
            return USAGE;
        }
    };

    if typed.global.version {
        return version(&typed.global);
    }
    let Some(command) = typed.command else {
        print!("{}", help::overall());
        return OK;
    };
    if typed.global.help {
        print!("{}", help::for_command(command));
        return OK;
    }

    if typed.global.dry_run {
        return explain(command, &typed);
    }
    if command.local {
        return locally(command, &typed);
    }
    remotely(command, typed)
}

/// The version, in whichever form was asked for.
///
/// Every command honours `--json`, and this one used to be the exception: it printed a sentence
/// whatever it was asked, so a program that passed `--json` to everything — which is what the
/// documentation tells an agent to do — got something it could not read. Found by the agent
/// assessment, which is what an assessment is for.
fn version(global: &Global) -> i32 {
    if global.json {
        say(&json!({
            "ok": true,
            "command": "version",
            "message": format!("quill-cli {VERSION}"),
            "result": { "version": VERSION },
        }));
    } else if !global.quiet {
        println!("quill-cli {VERSION}");
    }
    OK
}

/// Say what would be sent, and send nothing.
///
/// `clig.dev` asks for a `--dry-run` on anything that changes something, and this is Quill's: it
/// prints the command's wire name and every argument the way the window would receive them. It is
/// also the honest way to check that a command line means what somebody thought it meant, which is
/// what the agent assessment uses it for — and it needs no running Quill, so it can be used to
/// check a script before there is a window to run it against.
fn explain(command: &'static Command, typed: &Typed) -> i32 {
    let value = json!({
        "ok": true,
        "dryRun": true,
        "command": command.wire(),
        "name": command.typed(),
        "arguments": Value::Object(typed.arguments.clone()),
        "local": command.local,
    });
    if typed.global.json {
        say(&value);
    } else if !typed.global.quiet {
        println!("{} would be sent as {}", command.typed(), command.wire());
        for (name, given) in &typed.arguments {
            println!("  {name} = {given}");
        }
    }
    OK
}

/// The commands the client answers on its own, with no Quill involved.
fn locally(command: &'static Command, typed: &Typed) -> i32 {
    match command.wire().as_str() {
        "version" => version(&typed.global),
        "commands" => {
            let only = typed.arguments.get("name").and_then(Value::as_str);
            if let Some(name) = only {
                if quill_cli::catalogue::find(name).is_none() {
                    complain(&format!("There is no command called `{name}`."), &typed.global);
                    return USAGE;
                }
            }
            if typed.global.json {
                say(&help::as_json(only));
            } else {
                match only.and_then(quill_cli::catalogue::find) {
                    Some(command) => print!("{}", help::for_command(command)),
                    None => print!("{}", help::overall()),
                }
            }
            OK
        }
        "instances" => {
            let running = client::running();
            let value = json!({
                "count": running.len(),
                "instances": running.iter().map(describe).collect::<Vec<Value>>(),
            });
            if typed.global.json {
                say(&value);
            } else if running.is_empty() {
                println!("No Quill is running.");
            } else {
                for instance in &running {
                    println!(
                        "pid {:<8} port {:<6} {}",
                        instance.pid,
                        instance.port,
                        instance.folder.display()
                    );
                }
            }
            OK
        }
        "launch" => {
            let folder = typed
                .arguments
                .get("folder")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let timeout = typed
                .arguments
                .get("timeout")
                .and_then(Value::as_str)
                .and_then(|text| text.trim().parse().ok())
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_LAUNCH_TIMEOUT);
            let wait = !typed.arguments.contains_key("no-wait");
            match client::launch(&folder, timeout, wait) {
                Ok((instance, pid)) => {
                    let value = match &instance {
                        Some(instance) => describe(instance),
                        None => json!({ "pid": pid }),
                    };
                    if typed.global.json {
                        say(&json!({ "ok": true, "command": "launch", "result": value }));
                    } else if !typed.global.quiet {
                        match &instance {
                            Some(instance) => println!(
                                "Quill {} is running on port {} in {}",
                                instance.pid,
                                instance.port,
                                instance.folder.display()
                            ),
                            None => println!("Quill was started as process {pid}"),
                        }
                    }
                    OK
                }
                Err(problem) => unreachable_to_code(&problem, &typed.global),
            }
        }
        other => {
            complain(&format!("`{other}` is not answered by the client."), &typed.global);
            USAGE
        }
    }
}

/// Everything else: find a Quill, send the command, print what came back.
fn remotely(command: &'static Command, typed: Typed) -> i32 {
    let instance = match client::choose(typed.global.instance.as_deref()) {
        Ok(instance) => instance,
        Err(problem) => return unreachable_to_code(&problem, &typed.global),
    };
    let timeout = client_timeout(&typed);
    match client::ask(&instance, &command.wire(), typed.arguments.clone(), timeout) {
        Ok(reply) => report(&reply, &typed.global),
        Err(problem) => unreachable_to_code(&problem, &typed.global),
    }
}

/// How long the client waits for an answer.
///
/// Long enough for whatever the command itself was told to wait for. `terminal read --wait-for`
/// and `git action --wait` hold the answer open on purpose, and a client that gave up before the
/// window did would report a timeout for something that was about to work. Five seconds of slack
/// on top, so the window's own timeout is always the one that fires.
fn client_timeout(typed: &Typed) -> Duration {
    let asked = typed.global.timeout.map(Duration::from_millis).unwrap_or(DEFAULT_TIMEOUT);
    let waiting: u64 = ["timeout", "wait"]
        .iter()
        .filter_map(|name| typed.arguments.get(*name))
        .filter_map(|value| match value {
            Value::String(text) => text.trim().parse::<u64>().ok(),
            Value::Number(number) => number.as_u64(),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    asked.max(Duration::from_millis(waiting + 5_000))
}

/// Print a reply, and turn it into an exit code.
fn report(reply: &Reply, global: &Global) -> i32 {
    if global.json {
        say(&reply.to_json());
        return exit_for(reply);
    }
    if let Some(failure) = &reply.error {
        complain(&failure.message, global);
        return exit_for(reply);
    }
    if global.quiet {
        return OK;
    }
    if !reply.message.is_empty() {
        println!("{}", reply.message);
    }
    // A reply carries its own printable form when it has one: `text` for something that is text all
    // through, such as a document or a terminal screen, and `lines` for a listing. The client does
    // not lay anything out itself, because the window is the only one that knows what it is looking
    // at.
    if let Some(text) = reply.result.get("text").and_then(Value::as_str) {
        let mut out = std::io::stdout();
        let _ = out.write_all(text.as_bytes());
        if !text.ends_with('\n') {
            let _ = out.write_all(b"\n");
        }
        let _ = out.flush();
    } else if let Some(lines) = reply.result.get("lines").and_then(Value::as_array) {
        for line in lines {
            match line.as_str() {
                Some(line) => println!("{line}"),
                None => println!("{line}"),
            }
        }
    }
    OK
}

fn exit_for(reply: &Reply) -> i32 {
    let Some(failure) = &reply.error else {
        return OK;
    };
    match failure.code.as_str() {
        code::UNKNOWN_COMMAND | code::USAGE => USAGE,
        code::NOT_RUNNING | code::REFUSED => NOT_RUNNING,
        code::SEVERAL => SEVERAL,
        code::TIMED_OUT => TIMED_OUT,
        _ => REFUSED,
    }
}

fn unreachable_to_code(problem: &Unreachable, global: &Global) -> i32 {
    if global.json {
        say(&json!({
            "ok": false,
            "error": { "code": problem.code, "message": problem.message },
        }));
    } else {
        complain(&problem.message, global);
    }
    match problem.code {
        code::SEVERAL => SEVERAL,
        code::TIMED_OUT => TIMED_OUT,
        code::FAILED => REFUSED,
        _ => NOT_RUNNING,
    }
}

fn describe(instance: &Instance) -> Value {
    json!({
        "pid": instance.pid,
        "port": instance.port,
        "folder": instance.folder.to_string_lossy(),
        "started": instance.started,
    })
}

/// Print a value as JSON, laid out, on standard output.
fn say(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()));
}

/// Print a complaint on standard error, in red when the terminal will show it.
fn complain(message: &str, global: &Global) {
    if global.json {
        return;
    }
    if colour() {
        eprintln!("\u{1b}[31m{message}\u{1b}[0m");
    } else {
        eprintln!("{message}");
    }
}

/// Whether to colour anything.
///
/// The three rules `clig.dev` sets out: not when the output is not a terminal, not when `NO_COLOR`
/// is set to anything at all, and not when the terminal says it cannot. `--no-color` is read here
/// too, straight from the arguments, because it is the only global flag that changes nothing but
/// this and threading it through would be a field nothing else reads.
fn colour() -> bool {
    if std::env::args().any(|word| word == "--no-color") {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM").map(|term| term == "dumb").unwrap_or(false) {
        return false;
    }
    std::io::stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn help_and_version_work_with_no_quill_running() {
        assert_eq!(run(&words("--help")), OK);
        assert_eq!(run(&words("--version")), OK);
        assert_eq!(run(&words("commands --json")), OK);
    }

    #[test]
    fn a_command_line_that_will_not_parse_is_the_callers_mistake() {
        assert_eq!(run(&words("tab opne x")), USAGE);
        assert_eq!(run(&words("tab open")), USAGE);
        assert_eq!(run(&words("commands nonsense")), USAGE);
    }

    #[test]
    fn the_client_waits_at_least_as_long_as_the_command_was_told_to() {
        let typed = parse::parse(&words("terminal read --wait-for done --timeout 30000"))
            .expect("parses");
        assert!(
            client_timeout(&typed) >= Duration::from_millis(35_000),
            "the client must outlast the window's own wait"
        );
    }

    #[test]
    fn a_failure_becomes_the_exit_code_its_kind_deserves() {
        assert_eq!(exit_for(&Reply::failed("x", code::NOT_FOUND, "no")), REFUSED);
        assert_eq!(exit_for(&Reply::failed("x", code::USAGE, "no")), USAGE);
        assert_eq!(exit_for(&Reply::failed("x", code::TIMED_OUT, "no")), TIMED_OUT);
        assert_eq!(exit_for(&Reply::done("x", "yes", Value::Null)), OK);
    }
}
