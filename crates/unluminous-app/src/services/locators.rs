//! Turning a configuration that builds a program into the program itself.
//!
//! `task-1687` §13 deferred this and named it: "debugging a Cargo/npm configuration by deriving the
//! binary — Zed's locators. Right and wanted, and a design of its own." `task-1692` is that design,
//! and it exists because the refusal it replaces was the commonest way debugging did nothing at all:
//! `cargo run` is the configuration Unluminous's own project suggests, and pressing Debug on it said
//! *"cargo builds the program rather than being it"*.
//!
//! **Cargo is asked rather than guessed at.** `cargo build --message-format=json-render-diagnostics`
//! prints one JSON object a line, and a `compiler-artifact` object carries an `executable` field
//! that is `null` for a library and the full path for everything else. The last one with a path is
//! the program `cargo run` would have run. Deriving it from `target/debug/<crate>` by convention
//! instead is wrong for workspaces, examples, tests, custom profiles and renamed binaries — all of
//! which this repository has — and asking costs one process.
//!
//! **`cargo test` is the same with `--no-run`**, and the artifact is the test binary, which is how a
//! failing test is debugged.
//!
//! **npm gets no locator, deliberately.** js-debug takes a command line and runs it through its own
//! runtime, so `npm run dev` already debugs correctly; deriving anything there would be inventing a
//! problem to solve.
//!
//! Nothing here runs anything. This module translates a command line and reads cargo's output;
//! `UnluminousApp::begin_a_build` is what starts the process, on a thread, the way `unluminous-git` runs git.

use std::path::PathBuf;

use serde_json::Value;

use crate::services::run_configurations::split_command;

/// The flag that makes cargo describe what it built, in the form this module parses.
///
/// `json-render-diagnostics` rather than plain `json`: the machine-readable artifact lines still
/// arrive on standard output, and the compiler's errors are rendered for a person on standard error
/// instead of arriving as a second JSON shape nobody reads. That is what makes a failed build's
/// message the compiler's own words.
pub const MESSAGE_FORMAT: &str = "--message-format=json-render-diagnostics";

/// A build that has to happen before there is anything to debug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// The program to run, which is always the one the configuration named.
    pub program: String,
    /// Its arguments, with the subcommand rewritten and [`MESSAGE_FORMAT`] added.
    pub args: Vec<String>,
    /// The `--bin`, `--example` or `--test` name, which is what picks one artifact out of a
    /// workspace that built several.
    pub wanted: Option<String>,
    /// The debuggee's own arguments — everything after `--`, and a `cargo test` filter — which never
    /// reach the build command.
    pub program_args: Vec<String>,
    /// What the status bar and the debug tile say while it runs.
    pub what: String,
}

impl Build {
    /// The whole command line, as a person would read it back.
    pub fn command(&self) -> String {
        match self.args.is_empty() {
            true => self.program.clone(),
            false => format!("{} {}", self.program, self.args.join(" ")),
        }
    }
}

/// The build this command line needs first, or nothing when it is already a program.
///
/// One `match` on the tool and one on its subcommand. A cargo subcommand that produces no program —
/// `cargo fmt`, `cargo clippy` — is not a locator's business and answers `None`, which leaves the
/// registry's own refusal to say so.
pub fn locate(command: &str) -> Option<Build> {
    let tokens = split_command(command);
    let (program, rest) = tokens.split_first()?;
    let stem = std::path::Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match stem.as_str() {
        "cargo" => cargo(program, rest),
        _ => None,
    }
}

/// `cargo run` and `cargo test`, translated into the build that produces their binary.
///
/// | written | built | the debuggee gets |
/// |---|---|---|
/// | `cargo run` | `cargo build --message-format=…` | nothing |
/// | `cargo run --release -- --fast` | `cargo build --release --message-format=…` | `--fast` |
/// | `cargo test the_name` | `cargo test --no-run --message-format=…` | `the_name` |
fn cargo(program: &str, rest: &[String]) -> Option<Build> {
    // Everything after `--` belongs to the program rather than to cargo, on both subcommands.
    let separator = rest.iter().position(|token| token == "--");
    let (before, after) = match separator {
        Some(at) => (&rest[..at], rest[at + 1..].to_vec()),
        None => (rest, Vec::new()),
    };
    let subcommand = before.iter().find(|token| !token.starts_with('-'))?;
    let testing = match subcommand.as_str() {
        "run" | "r" => false,
        "test" | "t" => true,
        // `cargo build` as a run configuration is somebody who meant to build; every other
        // subcommand produces no program at all. Neither is a thing to debug.
        _ => return None,
    };
    let mut args: Vec<String> = Vec::new();
    let mut program_args: Vec<String> = Vec::new();
    let mut wanted: Option<String> = None;
    let mut seen_the_subcommand = false;
    let mut expecting: Option<String> = None;
    for token in before {
        // The subcommand itself: `run` becomes `build`, and `test` stays and gains `--no-run`.
        if !seen_the_subcommand && token == subcommand {
            seen_the_subcommand = true;
            args.push(match testing {
                true => "test".to_owned(),
                false => "build".to_owned(),
            });
            continue;
        }
        // The value of a flag that takes one, and the value of `--bin` is also what picks the
        // artifact out of a workspace that built several.
        if let Some(flag) = expecting.take() {
            if matches!(flag.as_str(), "--bin" | "--example" | "--test" | "--bench") {
                wanted = Some(token.clone());
            }
            args.push(token.clone());
            continue;
        }
        if token.starts_with('-') {
            if let Some((flag, value)) = token.split_once('=') {
                if matches!(flag, "--bin" | "--example" | "--test" | "--bench") {
                    wanted = Some(value.to_owned());
                }
            } else if TAKES_A_VALUE.contains(&token.as_str()) {
                expecting = Some(token.clone());
            }
            args.push(token.clone());
            continue;
        }
        // A bare word after `cargo test` is a filter the test binary takes, and after `cargo run` it
        // is not a thing cargo accepts at all — so either way it belongs to the program.
        program_args.push(token.clone());
    }
    if testing {
        args.push("--no-run".to_owned());
    }
    args.push(MESSAGE_FORMAT.to_owned());
    program_args.extend(after);
    let what = match &wanted {
        Some(name) => format!("Building {name}"),
        None => match testing {
            true => "Building the tests".to_owned(),
            false => "Building".to_owned(),
        },
    };
    Some(Build { program: program.to_owned(), args, wanted, program_args, what })
}

/// The cargo flags that take their value as the next word, which is what stops `--bin unluminous` being
/// read as a flag and a filter.
const TAKES_A_VALUE: &[&str] = &[
    "--bin", "--example", "--test", "--bench", "-p", "--package", "--features", "-F", "--profile",
    "--target", "--target-dir", "--manifest-path", "-j", "--jobs", "--config",
];

/// The program cargo said it built, out of everything it printed.
///
/// `wanted` is the `--bin` name when one was given. Without it the **last** artifact with an
/// executable is taken, which is the one cargo would have run: build scripts come first and the
/// thing being built comes last.
pub fn executable(output: &str, wanted: Option<&str>) -> Option<PathBuf> {
    let mut found: Option<PathBuf> = None;
    for line in output.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(path) = message["executable"].as_str() else {
            continue;
        };
        // A build script is an executable cargo built and never a program to debug.
        let kinds = message["target"]["kind"].as_array().cloned().unwrap_or_default();
        if kinds.iter().any(|kind| kind == "custom-build") {
            continue;
        }
        let name = message["target"]["name"].as_str().unwrap_or_default();
        match wanted {
            Some(asked) if asked != name => continue,
            _ => found = Some(PathBuf::from(path)),
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(command: &str) -> Build {
        locate(command).unwrap_or_else(|| panic!("{command} should need a build"))
    }

    #[test]
    fn cargo_run_becomes_cargo_build_and_asks_for_the_json() {
        let built = build("cargo run");
        assert_eq!(built.program, "cargo");
        assert_eq!(built.args, vec!["build".to_owned(), MESSAGE_FORMAT.to_owned()]);
        assert!(built.program_args.is_empty());
        assert_eq!(built.wanted, None);
    }

    #[test]
    fn the_flags_that_choose_what_is_built_are_kept_and_the_programs_arguments_are_not() {
        let built = build("cargo run --release -p unluminous-app --bin unluminous -- --control off");
        assert_eq!(
            built.args,
            vec![
                "build".to_owned(),
                "--release".to_owned(),
                "-p".to_owned(),
                "unluminous-app".to_owned(),
                "--bin".to_owned(),
                "unluminous".to_owned(),
                MESSAGE_FORMAT.to_owned(),
            ]
        );
        assert_eq!(built.program_args, vec!["--control".to_owned(), "off".to_owned()]);
        assert_eq!(built.wanted.as_deref(), Some("unluminous"), "--bin also picks the artifact");
        assert_eq!(built.what, "Building unluminous");
    }

    #[test]
    fn a_flag_written_with_an_equals_sign_is_read_the_same_way() {
        let built = build("cargo run --bin=unluminous --features=fast");
        assert_eq!(built.wanted.as_deref(), Some("unluminous"));
        assert!(built.args.contains(&"--features=fast".to_owned()));
    }

    /// `cargo test` keeps its subcommand, gains `--no-run`, and its filter goes to the test binary —
    /// which is what makes debugging one failing test work.
    #[test]
    fn cargo_test_builds_the_test_binary_and_the_filter_is_the_programs_own() {
        let built = build("cargo test the_name -p unluminous-core -- --nocapture");
        assert_eq!(
            built.args,
            vec![
                "test".to_owned(),
                "-p".to_owned(),
                "unluminous-core".to_owned(),
                "--no-run".to_owned(),
                MESSAGE_FORMAT.to_owned(),
            ]
        );
        assert_eq!(built.program_args, vec!["the_name".to_owned(), "--nocapture".to_owned()]);
        assert_eq!(built.what, "Building the tests");
    }

    /// A cargo subcommand that produces no program is not a locator's business, and neither is
    /// anything that is not cargo.
    #[test]
    fn a_subcommand_that_builds_nothing_and_a_program_that_is_one_need_no_build() {
        assert_eq!(locate("cargo fmt"), None);
        assert_eq!(locate("cargo clippy --fix"), None);
        assert_eq!(locate("target\\debug\\unluminous.exe"), None);
        assert_eq!(locate("node server.js"), None);
        assert_eq!(locate("npm run dev"), None, "js-debug runs npm itself");
        assert_eq!(locate(""), None);
    }

    #[test]
    fn the_command_reads_back_as_a_person_would_write_it() {
        assert_eq!(
            build("cargo run --release").command(),
            format!("cargo build --release {MESSAGE_FORMAT}")
        );
    }

    /// A transcript of the shape `cargo build --message-format=json` really prints, cut down to the
    /// three lines that matter: a build script, a library, and the binary.
    const TRANSCRIPT: &str = r#"{"reason":"compiler-artifact","target":{"kind":["custom-build"],"name":"build-script-build"},"executable":"C:\\p\\target\\debug\\build\\x-1\\build-script-build.exe"}
{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"unluminous_core"},"executable":null}
{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"unluminous"},"executable":"C:\\p\\target\\debug\\unluminous.exe"}
{"reason":"build-finished","success":true}"#;

    #[test]
    fn the_binary_is_taken_and_the_library_and_the_build_script_are_not() {
        let found = executable(TRANSCRIPT, None).expect("cargo built a binary");
        assert_eq!(found, PathBuf::from("C:\\p\\target\\debug\\unluminous.exe"));
    }

    #[test]
    fn a_named_bin_is_taken_out_of_a_workspace_that_built_several() {
        let several = format!(
            "{TRANSCRIPT}\n{}",
            r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"unluminous-cli"},"executable":"C:\\p\\target\\debug\\unluminous-cli.exe"}"#
        );
        assert_eq!(
            executable(&several, None),
            Some(PathBuf::from("C:\\p\\target\\debug\\unluminous-cli.exe")),
            "with nothing asked for, the last is the one cargo would have run"
        );
        assert_eq!(
            executable(&several, Some("unluminous")),
            Some(PathBuf::from("C:\\p\\target\\debug\\unluminous.exe")),
            "and --bin picks"
        );
        assert_eq!(executable(&several, Some("nothing-of-that-name")), None);
    }

    #[test]
    fn a_library_only_build_produces_nothing_rather_than_a_panic() {
        let library = r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"unluminous_core"},"executable":null}
{"reason":"build-finished","success":true}"#;
        assert_eq!(executable(library, None), None);
        assert_eq!(executable("not json at all\n", None), None);
        assert_eq!(executable("", None), None);
    }
}
