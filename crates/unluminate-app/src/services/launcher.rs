//! Starting another Unluminate.
//!
//! `tasks/improvements.md` asks for several Unluminate windows at once, each with its own project, the way
//! The reference editor does it. Each one is its own process rather than a second window inside this process.
//!
//! That is a decision worth recording. A second window in the same process would share the document, the
//! file tree, the settings in memory and the terminal sessions, so every one of those would have to learn
//! which window it belonged to. A second process shares nothing: it reads the same settings file, opens
//! its own project, and if it stops it takes nothing with it. Unluminate already takes the folder to open as
//! its first argument, which is all a second process needs, and the reference editor works the same way.

use std::path::Path;
use std::process::Command;

/// The command that starts another Unluminate on `folder`.
///
/// Split out from running it so that the arguments can be checked by a test without starting a window.
pub fn command_for(program: &Path, folder: &Path) -> Command {
    let mut command = Command::new(program);
    command.arg(folder);
    command
}

/// Start another Unluminate on `folder`.
///
/// The new process is not waited for and its output is left where this one's goes. Failing to start it is
/// reported and otherwise ignored, because the window that asked is still working.
pub fn open_window(folder: &Path) -> bool {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(problem) => {
            eprintln!("Unluminate could not find its own program to start another window: {problem}");
            return false;
        }
    };
    match command_for(&program, folder).spawn() {
        Ok(_) => true,
        Err(problem) => {
            eprintln!("Unluminate could not start another window: {problem}");
            false
        }
    }
}

/// The command that opens the platform's file manager with `path` selected.
///
/// Split out from running it for the same reason [`command_for`] is: a test can check what would be
/// run without a file manager window appearing on the machine running the tests.
pub fn reveal_command(path: &Path) -> Command {
    if cfg!(target_os = "windows") {
        // `/select,` and the path must be one argument with no space after the comma, which is why
        // this is built as a single string rather than as two arguments.
        let mut command = Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    } else {
        // Every desktop on Linux has its own file manager, and `xdg-open` on a folder is the closest
        // thing to a common answer. It opens the folder rather than selecting the file in it.
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    }
}

/// Show `path` in the platform's file manager.
///
/// The name of the entry says `Explorer` on Windows and `Finder` on macOS, because those are what
/// the thing is called there.
pub fn reveal(path: &Path) -> bool {
    match reveal_command(path).spawn() {
        Ok(_) => true,
        Err(problem) => {
            eprintln!("Unluminate could not show {} in the file manager: {problem}", path.display());
            false
        }
    }
}

/// What the entry that does it is called on this platform.
pub fn file_manager_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Open Containing Folder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn revealing_a_file_asks_the_platform_for_its_own_file_manager() {
        let command = reveal_command(Path::new("/tmp/notes/one.md"));
        let program = command.get_program().to_string_lossy().to_string();
        let arguments: Vec<String> =
            command.get_args().map(|arg| arg.to_string_lossy().to_string()).collect();
        if cfg!(target_os = "windows") {
            assert_eq!(program, "explorer");
            // One argument, with no space after the comma: `explorer` will not select the file if
            // the switch and the path are separate arguments.
            assert_eq!(arguments, vec!["/select,/tmp/notes/one.md".to_owned()]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            assert_eq!(arguments, vec!["-R".to_owned(), "/tmp/notes/one.md".to_owned()]);
        } else {
            assert_eq!(program, "xdg-open");
            assert_eq!(arguments, vec!["/tmp/notes".to_owned()]);
        }
    }

    #[test]
    fn the_command_runs_unluminate_again_with_the_folder_after_it() {
        let program = PathBuf::from("/somewhere/unluminate");
        let folder = PathBuf::from("/projects/book");
        let command = command_for(&program, &folder);
        assert_eq!(command.get_program(), program.as_os_str());
        let arguments: Vec<_> = command.get_args().collect();
        assert_eq!(arguments, vec![folder.as_os_str()], "the folder is the only argument");
    }

    /// A small program this machine is certain to have, that takes a path after it and stops straight
    /// away. `/bin/echo` is a Unix path and there is nothing at it on Windows, so the test asked the
    /// operating system to start a program that was not there and read the refusal as the plumbing being
    /// broken. `where` is the nearest thing Windows ships: it is in the folder `SystemRoot` names on every
    /// installation, and whatever it is handed it prints a line and exits.
    fn harmless_program() -> PathBuf {
        if cfg!(target_os = "windows") {
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
            PathBuf::from(system_root).join("System32").join("where.exe")
        } else {
            PathBuf::from("/bin/echo")
        }
    }

    /// Starting a real second window is what the `New Window` entry does, and this checks the plumbing
    /// under it without opening a window: `true` is only returned once a process has actually started.
    #[test]
    fn a_second_process_can_be_started() {
        // `std::env::current_exe` inside a test is the test binary, so this would run the tests again.
        // The command is built for a program that exists and does nothing instead.
        let program = harmless_program();
        let folder = std::env::temp_dir();
        let child = command_for(&program, &folder).spawn();
        assert!(child.is_ok(), "spawning a second process should work on this machine");
        // Wait for it, so the test leaves nothing behind for the operating system to collect.
        if let Ok(mut child) = child {
            child.wait().ok();
        }
    }
}
