//! The environment a person's shell gives a command, which is not the environment this process was
//! started with.
//!
//! A Quill started from the Finder or the Dock is started by launchd, and launchd gives it about a
//! dozen variables: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME`, `USER`, `SHELL` and little else.
//! Nothing `~/.zshrc` or `~/.bash_profile` sets is there. Two things broke on that, and both were
//! measured on a real window rather than reasoned about:
//!
//! - **A program installed under the home folder could not be found at all.** `claude` lives in
//!   `~/.local/bin`, so the Agent-Tasks board answered `claude could not be started: No such file or
//!   directory`, while typing `claude` in the terminal tile beside it worked.
//! - **A program that did start was not logged in.** `~/.zshrc` is where `ANTHROPIC_BASE_URL`,
//!   `ANTHROPIC_API_KEY` and the gateway's own `ANTHROPIC_CUSTOM_HEADERS` and `NODE_EXTRA_CA_CERTS`
//!   are set. An agent spawned straight from the window got none of them and said it was not logged
//!   in, and no amount of `PATH` would have fixed that.
//!
//! The terminal tile has never had either problem, because it starts the person's shell and the shell
//! reads their profile. So the profile is what is read here — `$SHELL -ilc 'env -0'` — and what a
//! program Quill starts gets is what a command typed in Quill's own terminal would have got. That is
//! the whole of the rule, and it is why this reads the **environment** rather than only the `PATH` it
//! was first written to read.
//!
//! Interactive as well as login, which is what the `-i` is for: `~/.zshrc` is only read by an
//! interactive shell, and on most machines that is where the exports are.
//!
//! `env -0` rather than `env`, because a value may hold a newline and NUL is the one byte it cannot
//! hold. The marker in front of it is what separates the answer from whatever the profile printed on
//! its own — a banner, a version notice, a warning about a stale cache — and it is written with a NUL
//! after it so a banner with no newline cannot run into the first variable.
//!
//! **`env` is named by its full path**, `/usr/bin/env`, and that is not tidiness. Measured on the
//! machine this was written on: `command -v env` answers `~/.local/bin/env`, something a tool installer
//! put there, and that one writes **nothing at all** for `-0` and exits 0 — so the read came back
//! holding the marker and no variables, and everything fell back to launchd's dozen without a word
//! being said about it. It is the same fault `services::debuggers` records about `tar` on Windows,
//! where a machine with Git for Windows ahead of it on `PATH` has a different `tar`: a program looked
//! up by name is whichever program that machine happens to find first.
//!
//! ## What is not carried over
//!
//! Four variables describe the shell that was asked rather than the machine, so they are dropped:
//! `PWD` and `OLDPWD`, which name the folder that shell was standing in and would contradict the
//! folder the child is actually started in; `SHLVL`, which counts how deep the shell was; and `_`,
//! which is the last command it ran. Everything else is carried, because deciding which of a person's
//! own exports matter to their own agent is not a decision Quill can make correctly — that is exactly
//! the guess a list of well known variables would be.
//!
//! ## A list of well known folders was the alternative, and it is a guess
//!
//! `services::debuggers::well_known` does keep such a list, and the difference is what it is looking
//! for: a debug adapter is a program somebody may never have had on `PATH` in any shell, and `claude`
//! is one they run by name every day. A list here would also have had nothing to say about the second
//! failure above, which is about credentials rather than about where a file is.
//!
//! ## It is read once, on a thread, at startup
//!
//! Measured on the machine this was written on it costs 1.38 seconds, which is a window that does not
//! appear for a second and a half if it is read before the window is built. [`start_reading`] is
//! called from `main`, and everything here blocks only if something asks before the read has finished
//! — `OnceLock::get_or_init` is what gives that for nothing.
//!
//! **Nothing runs a person's profile unless [`start_reading`] was called**, which only the released
//! binary does. That is the rule `load_settings` and `restore_project` already keep, and it matters
//! more here: a test that ran the profile would depend on the machine it ran on, would take a second
//! and a half doing it, and would put that person's real credentials into a test process.
//!
//! **Nothing is run on Windows.** Explorer starts a program with the person's own environment, so
//! what this process has there is already the whole of it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The environment the login shell reported, read at most once. `None` when there was no shell to ask
/// or it said nothing that could be read.
static LOGIN: OnceLock<Option<Vec<(String, String)>>> = OnceLock::new();

/// Whether anything has asked for the shell to be run. See the note at the top of the file.
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The variables that describe the shell that was asked rather than the machine it ran on.
const NOT_CARRIED: &[&str] = &["PWD", "OLDPWD", "SHLVL", "_"];

/// How long the shell is given. A profile that hangs — waiting on a network mount, asking for a
/// password — must not be a Quill that never starts an agent, so the read is given up on and what
/// this process already has is what is left. Ten seconds rather than two, because the read happens
/// once on a thread and a slow machine's profile is slow rather than broken.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// Start reading the login shell's environment on a thread, so the first program Quill starts does
/// not wait for it.
///
/// Called from `main` and by nothing else, which is the rule `load_settings` and `restore_project`
/// already keep: a test must not run the person's shell profile.
pub fn start_reading() {
    STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(|| {
        let _ = LOGIN.get_or_init(read_the_login_shell);
    });
}

/// What one variable is worth: the login shell's answer, then this process's own.
///
/// The shell first, because a Quill started from the Dock has the launchd value and the shell has the
/// person's. Where Quill was started from a terminal the two are the same value anyway.
pub fn variable(name: &str) -> Option<String> {
    let from_the_shell = login()
        .iter()
        .flat_map(|pairs| pairs.iter())
        .find(|(named, _)| named == name)
        .map(|(_, value)| value.clone());
    from_the_shell.or_else(|| std::env::var(name).ok())
}

/// The `PATH` a program is looked up on: the login shell's, with anything this process's own `PATH`
/// has that it does not, after it.
///
/// The process's own is kept rather than replaced because it is the honest answer when Quill was
/// started from a terminal — where it is the `PATH` the person typed `quill` on, which may be one
/// they have just changed for that one shell.
pub fn search_path() -> OsString {
    let from_the_shell = login()
        .iter()
        .flat_map(|pairs| pairs.iter())
        .find(|(name, _)| name == "PATH")
        .map(|(_, value)| value.clone());
    merge(from_the_shell.as_deref(), std::env::var_os("PATH").as_deref())
}

/// `PATH` and [`search_path`], as the pair a `quill_terminal::SessionSettings` lays over the
/// environment of a program Quill starts.
///
/// Separate from [`for_a_child`] because a run configuration wants its `PATH` mended without every
/// other variable a person's profile happens to set.
pub fn path_variable() -> (String, String) {
    ("PATH".to_owned(), search_path().to_string_lossy().into_owned())
}

/// Everything a program Quill starts should be given, which is what a command typed in a terminal
/// would have had.
///
/// The `PATH` in it is [`search_path`] rather than the shell's own, so a Quill started from a
/// terminal does not lose a folder that shell had added. The caller lays its own values over the top:
/// the board's gateway and key are the board's configuration and beat the profile, which is what the
/// Settings page says they do.
pub fn for_a_child() -> Vec<(String, String)> {
    let mut environment = vec![path_variable()];
    for (name, value) in login().iter().flat_map(|pairs| pairs.iter()) {
        if name != "PATH" && !NOT_CARRIED.contains(&name.as_str()) {
            environment.push((name.clone(), value.clone()));
        }
    }
    environment
}

/// Where `program` is, if it is anywhere on [`search_path`].
///
/// `PATH` is walked here rather than a `where`/`which` being run, because starting a process to find
/// out whether a process can be started is a round trip the window would wait for, and because
/// `where.exe` is not on every Windows.
pub fn find(program: &str) -> Option<PathBuf> {
    // A path with a separator in it was meant as a path rather than as a name to look up, which is
    // what a settings key holding a full path relies on.
    let named = Path::new(program);
    if named.components().count() > 1 {
        return named.is_file().then(|| named.to_path_buf());
    }
    look_up(program, &search_path())
}

/// The same, or a sentence saying what was looked for and where.
///
/// The sentence is the rule `services::debuggers` keeps about a missing adapter: a refusal naming the
/// program and the folders that were searched is one somebody can act on, and the operating system's
/// own `No such file or directory` — which is what a spawn by name gives back — is not.
pub fn required(program: &str) -> Result<PathBuf, String> {
    find(program).ok_or_else(|| {
        format!(
            "{program} is not installed, or is in a folder that is not on the PATH Quill searched: {}",
            search_path().to_string_lossy()
        )
    })
}

/// The same as [`find`], told which `PATH` to walk, so a test can hand in a folder of its own instead
/// of the machine's.
pub fn look_up(program: &str, path: &OsStr) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    for folder in std::env::split_paths(path).filter(|folder| !folder.as_os_str().is_empty()) {
        for spelling in spellings(program) {
            let candidate = folder.join(&spelling);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The spellings of one program name the platform will really run.
///
/// One on every platform but Windows, where a bare name is completed from `PATHEXT` — and where the
/// name as written is tried first, so a program named `claude.cmd` outright is not looked for as
/// `claude.cmd.COM`.
fn spellings(program: &str) -> Vec<String> {
    let mut spellings = vec![program.to_owned()];
    if cfg!(windows) {
        let listed = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
        for extension in listed.split(';').map(str::trim).filter(|extension| !extension.is_empty()) {
            spellings.push(format!("{program}{extension}"));
        }
    }
    spellings
}

/// The login shell's environment, blocking until it has been read if it is being read now, and empty
/// when nothing asked for it to be read at all.
fn login() -> Option<&'static Vec<(String, String)>> {
    if !STARTED.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    LOGIN.get_or_init(read_the_login_shell).as_ref()
}

/// `login` first, then every folder in `process` that `login` has not already named.
///
/// Pure, so the ordering and the removing of repeats are tested without a shell and without touching
/// the environment of the process running the tests.
fn merge(login: Option<&str>, process: Option<&OsStr>) -> OsString {
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut add = |path: &OsStr| {
        for folder in std::env::split_paths(path) {
            if !folder.as_os_str().is_empty() && !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    };
    if let Some(login) = login {
        add(OsStr::new(login));
    }
    if let Some(process) = process {
        add(process);
    }
    std::env::join_paths(folders).unwrap_or_else(|_| process.unwrap_or_default().to_owned())
}

/// What the marker in front of the variables says, so a banner the profile printed is not read as one.
const MARKER: &str = "QUILL-ENVIRONMENT";

/// The program that prints the environment, by its full path. See the note at the top of the file for
/// what a bare `env` turned out to be on the machine this was written on.
const ENV: &str = "/usr/bin/env";

/// Run the login shell and read the environment it reports.
#[cfg(not(windows))]
fn read_the_login_shell() -> Option<Vec<(String, String)>> {
    let shell = std::env::var("SHELL").ok().filter(|shell| !shell.trim().is_empty())?;
    let mut child = std::process::Command::new(&shell)
        .args(["-ilc", &format!("printf '{MARKER}\\0'; {ENV} -0")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    // Waited for by asking rather than by `wait`, so a profile that hangs is given up on instead of
    // holding this thread for the life of the process. The child is killed when the patience runs
    // out, or it would go on holding the pipe.
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < PATIENCE => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
    let mut said = Vec::new();
    std::io::Read::read_to_end(child.stdout.as_mut()?, &mut said).ok()?;
    parse(&said)
}

/// Nothing to run: Explorer starts a program with the person's own environment, so what this process
/// has on Windows is already the whole of it.
#[cfg(windows)]
fn read_the_login_shell() -> Option<Vec<(String, String)>> {
    None
}

/// The `NAME=value` records after the marker, which is what `printf` and `env -0` wrote.
///
/// `None` when the marker is not there, because that is a shell that could not be asked rather than a
/// person with no environment. A record with no `=` in it is not a variable and is dropped; bytes that
/// are not text are dropped with the variable they are in, since everything downstream of this is a
/// `String`.
fn parse(said: &[u8]) -> Option<Vec<(String, String)>> {
    let mut records = said.split(|byte| *byte == 0);
    records.find(|record| record.ends_with(MARKER.as_bytes()))?;
    let read: Vec<(String, String)> = records
        .filter_map(|record| std::str::from_utf8(record).ok())
        .filter_map(|record| record.split_once('='))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    // **The marker and no variables is not an answer.** That is what a machine whose `env` writes
    // nothing for `-0` gives back, and taking it as an empty environment would say the person's profile
    // sets nothing rather than that it could not be read. `None` means every reader falls back to what
    // this process already has, which is the honest answer.
    (!read.is_empty()).then_some(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(folders: &[&str]) -> OsString {
        std::env::join_paths(folders.iter().map(Path::new)).expect("a path with no separator in it")
    }

    /// What the shell really writes: a banner with no newline, the marker, then the variables.
    fn written(banner: &str, variables: &[&str]) -> Vec<u8> {
        let mut bytes = banner.as_bytes().to_vec();
        bytes.extend_from_slice(MARKER.as_bytes());
        for variable in variables {
            bytes.push(0);
            bytes.extend_from_slice(variable.as_bytes());
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn the_login_shells_folders_come_first_and_the_processes_own_are_kept_after_them() {
        let login = joined(&["/home/me/.local/bin", "/usr/bin"]);
        let process = joined(&["/usr/bin", "/sbin"]);
        let merged = merge(Some(&login.to_string_lossy()), Some(&process));
        let folders: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(
            folders,
            vec![
                PathBuf::from("/home/me/.local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/sbin"),
            ],
            "the login shell's order is kept, a repeat is named once, and nothing is lost"
        );
    }

    #[test]
    fn with_no_login_shell_the_answer_is_the_process_path_unchanged() {
        let process = joined(&["/usr/bin", "/bin"]);
        assert_eq!(merge(None, Some(&process)), process);
    }

    #[test]
    fn the_variables_are_read_after_the_marker_and_a_banner_in_front_of_it_is_ignored() {
        let said = written("Welcome to this machine, no newline after this", &[
            "PATH=/home/me/.local/bin:/usr/bin",
            "ANTHROPIC_API_KEY=a-key",
        ]);
        let read = parse(&said).expect("the marker is there");
        assert_eq!(read, vec![
            ("PATH".to_owned(), "/home/me/.local/bin:/usr/bin".to_owned()),
            ("ANTHROPIC_API_KEY".to_owned(), "a-key".to_owned()),
        ]);
    }

    #[test]
    fn a_value_with_a_newline_in_it_survives_because_the_records_are_separated_by_nul() {
        let said = written("", &["NODE_OPTIONS=--one\n--two", "AFTER=yes"]);
        let read = parse(&said).expect("the marker is there");
        assert_eq!(read, vec![
            ("NODE_OPTIONS".to_owned(), "--one\n--two".to_owned()),
            ("AFTER".to_owned(), "yes".to_owned()),
        ]);
    }

    #[test]
    fn a_shell_that_printed_no_marker_is_a_shell_that_could_not_be_asked() {
        assert_eq!(parse(b"Welcome to this machine\n"), None);
        assert_eq!(parse(b""), None);
    }

    #[test]
    fn the_marker_and_no_variables_is_not_an_answer_either() {
        // What an `env` that writes nothing for `-0` gives back, which is a real machine and is why
        // `ENV` is a full path. Read as an empty environment it would have said the profile sets
        // nothing.
        assert_eq!(parse(&written("", &[])), None);
    }

    #[test]
    fn a_record_that_is_not_a_variable_is_dropped_rather_than_guessed_at() {
        let said = written("", &["not a variable at all", "=novalue", "GOOD=yes"]);
        let read = parse(&said).expect("the marker is there");
        assert_eq!(read, vec![("GOOD".to_owned(), "yes".to_owned())]);
    }

    #[test]
    fn a_program_is_found_in_a_folder_on_the_path_and_a_missing_one_is_not() {
        let folder = std::env::temp_dir().join("quill-login-shell-test");
        std::fs::create_dir_all(&folder).expect("a folder under the temporary folder");
        let program = folder.join(match cfg!(windows) {
            true => "quill-test-agent.exe",
            false => "quill-test-agent",
        });
        std::fs::write(&program, b"").expect("a file that stands in for a program");
        let path = joined(&[&folder.to_string_lossy()]);
        assert_eq!(look_up("quill-test-agent", &path).as_deref(), Some(program.as_path()));
        assert!(look_up("quill-no-such-agent", &path).is_none());
        assert!(look_up("", &path).is_none(), "an empty name is not a program");
    }

    #[test]
    fn a_name_with_a_separator_in_it_is_a_path_rather_than_something_to_look_up() {
        assert!(find("/definitely/not/here/claude").is_none());
        assert!(find("quill-no-such-program-anywhere").is_none());
    }

    #[test]
    fn a_program_that_is_not_there_is_refused_by_name_with_the_folders_that_were_searched() {
        let problem = required("quill-no-such-agent").expect_err("nothing of that name exists");
        assert!(problem.starts_with("quill-no-such-agent is not installed"), "{problem}");
        assert!(
            problem.contains(&search_path().to_string_lossy().into_owned()),
            "the refusal has to say where it looked, or there is nothing to act on: {problem}"
        );
    }

    #[test]
    fn the_search_path_holds_everything_the_process_was_started_with() {
        let searched: Vec<PathBuf> = std::env::split_paths(&search_path()).collect();
        for folder in std::env::var_os("PATH").iter().flat_map(std::env::split_paths) {
            if !folder.as_os_str().is_empty() {
                assert!(
                    searched.contains(&folder),
                    "{} is on this process's PATH and has to be searched",
                    folder.display()
                );
            }
        }
    }

    #[test]
    fn what_a_child_is_given_always_names_the_path_and_never_the_shells_own_folder() {
        let given = for_a_child();
        assert_eq!(given.first().map(|(name, _)| name.as_str()), Some("PATH"));
        for (name, _) in &given {
            assert!(
                !NOT_CARRIED.contains(&name.as_str()),
                "{name} describes the shell that was asked rather than the machine"
            );
        }
    }
}
