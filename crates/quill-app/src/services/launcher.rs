//! Starting another Quill.
//!
//! `tasks/improvements.md` asks for several Quill windows at once, each with its own project, the way
//! IntelliJ does it. Each one is its own process rather than a second window inside this process.
//!
//! That is a decision worth recording. A second window in the same process would share the document, the
//! file tree, the settings in memory and the terminal sessions, so every one of those would have to learn
//! which window it belonged to. A second process shares nothing: it reads the same settings file, opens
//! its own project, and if it stops it takes nothing with it. Quill already takes the folder to open as
//! its first argument, which is all a second process needs, and IntelliJ works the same way.

use std::path::Path;
use std::process::Command;

/// The command that starts another Quill on `folder`.
///
/// Split out from running it so that the arguments can be checked by a test without starting a window.
pub fn command_for(program: &Path, folder: &Path) -> Command {
    let mut command = Command::new(program);
    command.arg(folder);
    command
}

/// Start another Quill on `folder`.
///
/// The new process is not waited for and its output is left where this one's goes. Failing to start it is
/// reported and otherwise ignored, because the window that asked is still working.
pub fn open_window(folder: &Path) -> bool {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(problem) => {
            eprintln!("Quill could not find its own program to start another window: {problem}");
            return false;
        }
    };
    match command_for(&program, folder).spawn() {
        Ok(_) => true,
        Err(problem) => {
            eprintln!("Quill could not start another window: {problem}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_command_runs_quill_again_with_the_folder_after_it() {
        let program = PathBuf::from("/somewhere/quill");
        let folder = PathBuf::from("/projects/book");
        let command = command_for(&program, &folder);
        assert_eq!(command.get_program(), program.as_os_str());
        let arguments: Vec<_> = command.get_args().collect();
        assert_eq!(arguments, vec![folder.as_os_str()], "the folder is the only argument");
    }

    /// Starting a real second window is what the `New Window` entry does, and this checks the plumbing
    /// under it without opening a window: `true` is only returned once a process has actually started.
    #[test]
    fn a_second_process_can_be_started() {
        // `std::env::current_exe` inside a test is the test binary, so this would run the tests again.
        // The command is built for a program that exists and does nothing instead.
        let program = PathBuf::from("/bin/echo");
        let folder = std::env::temp_dir();
        let started = command_for(&program, &folder).spawn().is_ok();
        assert!(started, "spawning a second process should work on this machine");
    }
}
