//! Reading and writing a secret in this machine's own keychain.
//!
//! **A secret is never written into a settings file.** The board being replaced keeps its four secrets in the
//! macOS keychain and its own notes say why: a settings file is copied between machines, readable by anything
//! that can read the folder, and pasted into a bug report. What Agent-Tasks writes down is the **name** of a
//! keychain entry; what is in it never touches disk through Quill.
//!
//! ## The platform's own tool, not a dependency
//!
//! `security` on macOS and `secret-tool` on Linux. Each is the tool the platform already ships, driven the way
//! `quill_git` drives the machine's real git and for the same reason: the machine's own keychain has the
//! machine's own unlock rules, its own prompts and its own audit, and a crate that reimplemented any of that
//! would be a second answer to a question the operating system has already answered.
//!
//! **There is no Windows keychain here.** Quill runs on Windows and has a Windows screenshot baseline, so this
//! is a gap rather than a platform Quill does not care about. An earlier version of this comment said
//! PowerShell's credential store was used on Windows; no such code was ever written, and a comment claiming a
//! secret is in a keychain when it is not is worse than no comment. On Windows, and on any other platform,
//! `read` answers `None` and `write` refuses with a sentence saying so, and the Settings page repeats it.
//! Writing it would mean driving `[Windows.Security.Credentials.PasswordVault]` through PowerShell, which
//! cannot be tested from this machine, and untested code that handles a secret is not worth having.
//!
//! **Nothing here prints or logs a secret.** A read that fails answers `None` and says nothing about why, and
//! a write reports success or a sentence naming the tool rather than the value. The value is handed to the
//! tool on its standard input rather than on its command line, so it never appears in a process list.
//!
//! On a platform with no tool, both functions answer as though there were no entry. That is the honest
//! answer: the board then launches its agents with their own credentials, which is what they do anyway.

use std::io::Write;
use std::process::{Command, Stdio};

/// What the entries are filed under, so Agent-Tasks' own are told apart from everything else on the machine.
pub const SERVICE: &str = "quill-agent-tasks";

/// The secret called `name`, or `None` when there is none and when the platform has no keychain.
///
/// Called at the moment an agent is launched and never held, so the value is in this process for as long as it
/// takes to hand it to a child.
pub fn read(name: &str) -> Option<String> {
    if !is_a_safe_name(name) {
        return None;
    }
    let output = tool_for_reading(name)?.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let secret = String::from_utf8(output.stdout).ok()?;
    let secret = secret.trim_end_matches(['\n', '\r']).to_owned();
    match secret.is_empty() {
        true => None,
        false => Some(secret),
    }
}

/// Write the secret called `name`, replacing whatever was there.
///
/// The value goes in on standard input, so it is not in a process list. An empty value **removes** the entry,
/// because clearing a field in the Settings page has to be able to mean "there is no key" rather than "the key
/// is the empty string".
pub fn write(name: &str, secret: &str) -> Result<(), String> {
    if !is_a_safe_name(name) {
        return Err(format!(
            "`{name}` is not a name a keychain entry can have: letters, digits, a dash, a dot or an underscore"
        ));
    }
    if secret.is_empty() {
        return remove(name);
    }
    let Some(mut command) = tool_for_writing(name) else {
        // Named, so somebody reading it knows whether to expect this to change. On Windows it is a gap; on a
        // platform with neither `security` nor `secret-tool` it is the honest answer.
        return Err(format!(
            "Quill has no keychain on {}: the key can be set for the agent in the environment it is launched              with instead, and nothing is written to disk by Quill either way",
            std::env::consts::OS
        ));
    };
    let mut running = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|problem| format!("the keychain tool could not be run: {problem}"))?;
    running
        .stdin
        .as_mut()
        .ok_or_else(|| "the keychain tool took no input".to_owned())?
        .write_all(secret.as_bytes())
        .map_err(|problem| format!("the secret could not be handed over: {problem}"))?;
    let finished = running
        .wait_with_output()
        .map_err(|problem| format!("the keychain tool did not finish: {problem}"))?;
    match finished.status.success() {
        true => Ok(()),
        // The tool's own words, which name the entry and never the value.
        false => Err(format!(
            "the keychain refused it: {}",
            String::from_utf8_lossy(&finished.stderr).trim()
        )),
    }
}

/// Take the secret called `name` out of the keychain.
pub fn remove(name: &str) -> Result<(), String> {
    if !is_a_safe_name(name) {
        return Ok(());
    }
    let Some(mut command) = tool_for_removing(name) else {
        return Ok(());
    };
    // A entry that was not there is not an error: clearing a field that was already empty asked for nothing.
    let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
    Ok(())
}

/// True when there is a secret called `name`, without reading it.
///
/// What the Settings page draws: `set` or `not set`, because a page that showed the value would be a page
/// somebody screenshots.
pub fn is_set(name: &str) -> bool {
    read(name).is_some()
}

/// Whether this name can be handed to a command line tool at all.
///
/// The name comes from a settings file somebody can edit, and it becomes an argument to a program. Letters,
/// digits, a dash, a dot and an underscore are what a keychain entry is called, and nothing else gets through
/// — which is what stops a name being read as another argument or as a shell fragment.
fn is_a_safe_name(name: &str) -> bool {
    // A leading dash is refused as well: `-w` is made of allowed characters and every one of these tools would
    // read it as one of its own flags rather than as the name of an entry.
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('-')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_'))
}

#[cfg(target_os = "macos")]
fn tool_for_reading(name: &str) -> Option<Command> {
    let mut command = Command::new("security");
    command.args(["find-generic-password", "-s", SERVICE, "-a", name, "-w"]);
    Some(command)
}

#[cfg(target_os = "macos")]
fn tool_for_writing(name: &str) -> Option<Command> {
    let mut command = Command::new("security");
    // `-U` replaces an entry that is already there, and `-w` with no value takes the value from standard
    // input, which is what keeps it out of the process list.
    command.args(["add-generic-password", "-s", SERVICE, "-a", name, "-U", "-w"]);
    Some(command)
}

#[cfg(target_os = "macos")]
fn tool_for_removing(name: &str) -> Option<Command> {
    let mut command = Command::new("security");
    command.args(["delete-generic-password", "-s", SERVICE, "-a", name]);
    Some(command)
}

#[cfg(target_os = "linux")]
fn tool_for_reading(name: &str) -> Option<Command> {
    let mut command = Command::new("secret-tool");
    command.args(["lookup", "service", SERVICE, "account", name]);
    Some(command)
}

#[cfg(target_os = "linux")]
fn tool_for_writing(name: &str) -> Option<Command> {
    let mut command = Command::new("secret-tool");
    command.args(["store", "--label", SERVICE, "service", SERVICE, "account", name]);
    Some(command)
}

#[cfg(target_os = "linux")]
fn tool_for_removing(name: &str) -> Option<Command> {
    let mut command = Command::new("secret-tool");
    command.args(["clear", "service", SERVICE, "account", name]);
    Some(command)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn tool_for_reading(_name: &str) -> Option<Command> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn tool_for_writing(_name: &str) -> Option<Command> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn tool_for_removing(_name: &str) -> Option<Command> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_could_be_read_as_something_else_is_refused() {
        // The name comes out of a settings file somebody can edit and becomes an argument to a program.
        for refused in [
            "",
            "-w",
            "a b",
            "a;rm -rf /",
            "a$(whoami)",
            "a\nb",
            "../../etc/passwd",
            "a'b",
            &"a".repeat(129),
        ] {
            assert!(!is_a_safe_name(refused), "`{refused}` should be refused");
        }
        for allowed in ["iliad", "anthropic-key", "openai.api_key", "a-1_2.3"] {
            assert!(is_a_safe_name(allowed), "`{allowed}` should be allowed");
        }
    }

    #[test]
    fn reading_a_name_that_is_not_there_answers_nothing_rather_than_failing() {
        // Every platform answers this the same way, including one with no keychain at all: the board then
        // launches its agents with their own credentials, which is what they do anyway.
        assert_eq!(read("quill-agent-tasks-no-such-entry-ever"), None);
        assert!(!is_set("quill-agent-tasks-no-such-entry-ever"));
    }

    #[test]
    fn a_refused_name_is_never_handed_to_the_tool() {
        // The refusal has to happen before the program is run, or the check is decoration.
        let problem = write("a b", "secret").expect_err("a name with a space");
        assert!(problem.contains("not a name"), "{problem}");
        // Removing one is not an error, because clearing a field that was already empty asked for nothing.
        assert!(remove("a b").is_ok());
    }
}
