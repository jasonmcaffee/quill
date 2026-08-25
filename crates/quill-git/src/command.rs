//! Running `git` and reading what it said.
//!
//! Every call in this crate goes through here, and every one of them returns an [`Outcome`] whether
//! it worked or not, holding git's own standard output and standard error. Nothing invents a message
//! of its own for a failure. A rejected push, a merge conflict, a detached HEAD and a missing
//! upstream all have good messages already, written by people who know exactly what happened, and
//! replacing them with "could not push" would be a step backwards.
//!
//! Output is read as bytes and turned into text with `from_utf8_lossy`. A file name on Windows can
//! hold anything the file system allows, and a path Quill cannot spell is still a path it should be
//! able to list.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// What a git command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// True when git exited with a status of zero.
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    /// Git's own message, which is what the window shows when something goes wrong.
    ///
    /// Standard error first, because that is where git explains itself, then standard output, which
    /// carries the summary of what a successful command did.
    pub fn message(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let stderr = self.stderr.trim();
        let stdout = self.stdout.trim();
        if !stderr.is_empty() {
            parts.push(stderr);
        }
        if !stdout.is_empty() {
            parts.push(stdout);
        }
        parts.join("\n")
    }

    /// The first line of the message, which is what fits in the status bar.
    pub fn summary(&self) -> String {
        self.message().lines().next().unwrap_or_default().to_owned()
    }

    /// A failure that never reached git at all: it is not installed, or the folder has gone.
    pub fn failed_to_run(problem: &std::io::Error) -> Self {
        Self { ok: false, stdout: String::new(), stderr: format!("git could not be run: {problem}") }
    }
}

/// Run `git` in `folder` with `arguments`.
///
/// The working directory is set rather than `-C` being passed, so that a caller reading the command
/// back sees the same thing git sees.
pub fn run<S: AsRef<OsStr>>(folder: &Path, arguments: &[S]) -> Outcome {
    let mut command = Command::new("git");
    command.current_dir(folder);
    for argument in arguments {
        command.arg(argument);
    }
    // Git will not open an editor or ask for a password on the terminal from inside Quill: there is
    // no terminal for it to ask on, and a git that sits waiting for an answer nobody can give would
    // hang the worker thread for ever. A credential helper still works, because that is a program of
    // its own with a window of its own.
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_EDITOR", "true");
    #[cfg(target_os = "windows")]
    {
        // Do not flash a console window for each command. 0x08000000 is CREATE_NO_WINDOW.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    match command.output() {
        Ok(output) => Outcome {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(problem) => Outcome::failed_to_run(&problem),
    }
}

/// Whether there is a `git` on this machine at all, and which version it is.
///
/// Asked once when a window starts. With no git, every git entry is dimmed and the status bar says
/// why, rather than each operation failing separately with a message about a program that is not
/// there.
pub fn version() -> Option<String> {
    let outcome = run(Path::new("."), &["--version"]);
    outcome.ok.then(|| outcome.stdout.trim().to_owned())
}

/// Split output that was asked for with `-z`, which separates its records with a zero byte.
///
/// Used wherever a path could be in the output. Git's ordinary output quotes and escapes a path with
/// a space or a newline in it, and unpicking that is a parser nobody should have to write; the zero
/// byte cannot appear in a path on either platform, so there is nothing to escape and nothing to
/// unpick.
pub fn split_nul(text: &str) -> Vec<&str> {
    text.split('\0').filter(|part| !part.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_puts_what_git_explained_first() {
        let outcome = Outcome {
            ok: false,
            stdout: "  To github.com:me/thing\n".to_owned(),
            stderr: " ! [rejected] main -> main (fetch first)\n".to_owned(),
        };
        assert_eq!(outcome.summary(), "! [rejected] main -> main (fetch first)");
        assert!(outcome.message().contains("To github.com:me/thing"));
    }

    #[test]
    fn a_message_with_nothing_in_it_is_empty_rather_than_blank_lines() {
        let outcome = Outcome { ok: true, stdout: "\n".to_owned(), stderr: String::new() };
        assert_eq!(outcome.message(), "");
        assert_eq!(outcome.summary(), "");
    }

    #[test]
    fn zero_separated_records_are_split_and_the_trailing_empty_one_dropped() {
        assert_eq!(split_nul("one\0two\0three\0"), vec!["one", "two", "three"]);
        assert_eq!(split_nul(""), Vec::<&str>::new());
        // A space in a path is left alone, which is the whole reason for asking for `-z`.
        assert_eq!(split_nul("my notes.md\0"), vec!["my notes.md"]);
    }
}
